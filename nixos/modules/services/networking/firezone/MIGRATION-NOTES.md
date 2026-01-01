# Firezone Portal Migration Notes

## Overview

This document describes the migration from Firezone's old umbrella architecture (Domain.*, Web.*, API.*) to the new unified Portal.* architecture.

**Migration Date**: 2026-01-01
**Firezone Version**: Commit `96ca73bf827339cdae2258cf64230fd0407f29f6`
**Key Change**: Three separate services consolidated into single `portal` service

## Architecture Changes

### Service Consolidation

**Old Architecture** (umbrella project):
- `firezone-server-domain` - Backend domain logic, migrations, background jobs
- `firezone-server-web` - Phoenix web interface
- `firezone-server-api` - WebSocket API for clients/gateways

**New Architecture** (single app):
- `firezone-server-portal` - All functionality in one application
- Single Erlang node: `portal@localhost.localdomain`
- Single HTTP port (default: 8080)

### Module Renames

| Old (Domain.*) | New (Portal.*) | Notes |
|---|---|---|
| `Domain.Accounts` | `Portal.Account` | Now schema, not context module |
| `Domain.Actors` | `Portal.Actor` | Email stored directly on schema |
| `Domain.Auth` | `Portal.Auth` | Simplified authentication |
| `Domain.Gateways` | `Portal.Site` | **Renamed concept** |
| `Domain.Relays` | `Portal.Relay` | **Now ephemeral** (not persisted) |
| `Domain.Resources` | `Portal.Resource` | Uses `site_id` instead of connections |
| `Domain.Policies` | `Portal.Policy` | Uses `group_id` not `actor_group_id` |
| `Domain.Tokens` | Multiple types | `ClientToken`, `GatewayToken`, `RelayToken` |

## Critical Database Constraints

### Actor Type Email Constraint

**Check constraint**: `type_is_valid`

```elixir
# account_user and account_admin_user MUST have email
# service_account and api_client MUST NOT have email (NULL)
(type IN ('account_user', 'account_admin_user') AND email IS NOT NULL)
OR (type IN ('service_account', 'api_client') AND email IS NULL)
```

**Impact**: When creating actors in provision.exs, email must be conditionally set based on type.

### Composite Primary Keys

Several schemas use composite primary keys:

- `Portal.Account` - single `id` (standard)
- `Portal.Actor` - composite `[:account_id, :id]`
- `Portal.ClientToken` - composite `[:account_id, :id]`
- `Portal.Site` - composite `[:account_id, :id]`
- `Portal.Resource` - composite `[:account_id, :id]`

**Impact**: Cannot use `Repo.get(Schema, id)` - must use `Repo.get_by(Schema, account_id: x, id: y)`

### Required Fields

**Portal.Account**:
- `legal_name` - required (new field, defaults to `name` in provision.exs)

**Portal.Resource**:
- `site_id` - replaces old `connections` array (one-to-one relationship)

## Provisioning Changes

### JSON Schema Compatibility

**CRITICAL**: The provision-state.json schema is **unchanged** for backward compatibility:

```json
{
  "accounts": {
    "my-account": {
      "gatewayGroups": { ... },    // Still named this (maps to Sites internally)
      "relayGroups": { ... },       // Deprecated (ignored with warning)
      "groups": { ... },            // Same
      "actors": { ... },            // Same
      "resources": {
        "res1": {
          "gatewayGroups": ["site1"] // Array format kept, but only first used
        }
      }
    }
  }
}
```

### Key Provisioning Behaviors

1. **Relay Groups**: Ignored with warning (relays are now global/ephemeral)
2. **Gateway Groups**: JSON key unchanged, internally creates `Portal.Site` records
3. **Resources**: Take only first gateway group from array as `site_id`
4. **Default Auth Provider**: Userpass provider auto-created for new accounts
5. **Temporary Admin**: No token/identity persisted (synthetic subject only)

### Important Implementation Details

**Resource Creation** (provision.exs:573-576):
```elixir
resource = %Resource{account_id: account.id}
  |> Ecto.Changeset.change(resource_attrs)
  |> Resource.changeset()  # Uses module's changeset for proper filter handling
  |> repo.insert!()
```

**Actor Creation** (provision.exs:396-405):
```elixir
actor_type = String.to_existing_atom(actor_data["type"])
actor_attrs = case actor_type do
  type when type in [:service_account, :api_client] ->
    %{account_id: account.id, type: type, name: actor_data["name"]}
  _ ->
    %{account_id: account.id, type: actor_type, name: actor_data["name"],
      email: actor_data["email"]}
end
```

**Default Auth Provider** (provision.exs:315-327):
```elixir
# Required for token creation to work
provider_id = Ecto.UUID.generate()
repo.insert!(%AuthProvider{
  id: provider_id,
  account_id: account.id,
  type: :userpass
})
repo.insert!(%Portal.Userpass.AuthProvider{
  id: provider_id,
  account_id: account.id,
  name: "Username & Password"
})
```

## NixOS Module Changes

### Configuration Changes

**Old**:
```nix
services.firezone.server = {
  domain.externalUrl = "...";
  web.externalUrl = "https://example.com/";
  api.externalUrl = "https://example.com/api/";
};
```

**New**:
```nix
services.firezone.server = {
  portal.externalUrl = "https://example.com/";
  # api.externalUrl removed - API served from same URL
};
```

### Service Changes

**Old systemd services**:
- `firezone-server-domain.service`
- `firezone-server-web.service`
- `firezone-server-api.service`

**New systemd service**:
- `firezone-server-portal.service`

### Cluster Configuration

**Old** (3 nodes):
```nix
clusterHosts = [
  "domain@localhost.localdomain"
  "web@localhost.localdomain"
  "api@localhost.localdomain"
];
```

**New** (1 node):
```nix
clusterHosts = [
  "portal@localhost.localdomain"
];
```

## Testing

### Running Tests

```bash
nix-build -A nixosTests.firezone
```

**Important**: Tests may take 30+ minutes to timeout if services fail to start.

### Test Changes

**Modified files**:
1. `nixos/tests/firezone/firezone.nix` - Updated service references
2. `nixos/tests/firezone/create-tokens.exs` - Migrated to Portal.* modules

**Key test updates**:
- Service reference: `firezone-server-domain` → `firezone-server-portal`
- External URLs: Consolidated to single `portal.externalUrl`
- Token creation: Updated for new token schemas and composite keys

## Package Structure

### Package Files

```
pkgs/by-name/fi/
├── firezone-server/           # Base package (mixReleaseName parameterized)
│   ├── package.nix
│   ├── 0000-add-mua.patch
│   └── 0001-remove-hardcoded-domain-config.patch
├── firezone-server-portal/    # Wrapper (mixReleaseName = "portal")
│   └── package.nix
├── firezone-server-domain/    # Legacy wrapper (also uses "portal")
│   └── package.nix
├── firezone-server-web/       # Legacy wrapper (also uses "portal")
│   └── package.nix
└── firezone-server-api/       # Legacy wrapper (also uses "portal")
    └── package.nix
```

**Note**: Old domain/web/api wrappers kept for compatibility but all use "portal" release.

### Build Configuration

**Key package.nix settings**:
```nix
{
  version = "0-unstable-2025-12-31";
  rev = "96ca73bf827339cdae2258cf64230fd0407f29f6";
  hash = "sha256-Q0NqBaQRGI4EwOcuYOrY66q8fux/rh4H67iFYBcuZGE=";

  mixReleaseName = "portal";  # Changed from "domain"

  # Asset configuration
  pnpmRoot = "assets";
  sourceRoot = "elixir/assets";  # Important for pnpm hash

  preBuild = ''
    config :portal, run_manual_migrations: true  # Changed from :domain
  '';
}
```

## Common Issues & Solutions

### Issue: "type_is_valid constraint violation"

**Cause**: Creating service_account/api_client with email field
**Solution**: Conditionally omit email for these types (see Actor Creation above)

### Issue: "legal_name violates not-null constraint"

**Cause**: Portal.Account requires legal_name field
**Solution**: Default to account name: `Map.put_new(attrs, :legal_name, attrs[:name])`

### Issue: "Repo.get/2 requires exactly one primary key"

**Cause**: Using `Repo.get/2` on schemas with composite keys
**Solution**: Use `Repo.get_by/3` with both key fields

### Issue: "Portal.Resource.Filter.changeset/2 is undefined"

**Cause**: Using `cast_embed(:filters)` without custom changeset
**Solution**: Use `Resource.changeset()` which includes proper filter handling

### Issue: "expected at least one result" (auth provider)

**Cause**: No auth provider created for new accounts
**Solution**: Auto-create Userpass provider when creating new account

## Future Maintenance

### When Updating Firezone Version

1. Check for schema changes in `firezone/elixir/lib/portal/*.ex`
2. Review migrations in `firezone/elixir/priv/repo/migrations/`
3. Compare `firezone/elixir/priv/repo/seeds.exs` for new patterns
4. Update hashes in package.nix:
   - `fetchFromGitHub.hash`
   - `pnpmDeps.hash`
   - `mixFodDeps.hash`
5. Test provisioning with existing provision-state.json files

### Key Files to Monitor

- `firezone-new/elixir/lib/portal/account.ex` - Account schema/constraints
- `firezone-new/elixir/lib/portal/actor.ex` - Actor types/constraints
- `firezone-new/elixir/lib/portal/resource.ex` - Resource validation
- `firezone-new/elixir/lib/portal/auth.ex` - Token encoding/subjects
- `firezone-new/elixir/priv/repo/migrations/` - Database constraints

### Useful Reference Commands

```bash
# Compare old vs new structures
diff -r firezone-old/elixir/apps/domain/lib/domain/ \
        firezone-new/elixir/lib/portal/

# Check for new migrations
ls -la firezone-new/elixir/priv/repo/migrations/

# Find schema changes
grep -r "schema \"" firezone-new/elixir/lib/portal/
```

## References

- **Old Firezone**: `firezone-old/` directory (for comparison)
- **New Firezone**: `firezone-new/` directory (current master)
- **Seeds File**: `firezone-new/elixir/priv/repo/seeds.exs` (canonical examples)
- **Migration Plan**: `.claude/plans/buzzing-imagining-marshmallow.md`

## Questions?

If encountering issues not covered here:

1. Check the seeds.exs file for canonical examples
2. Compare with old Domain.* implementation in firezone-old/
3. Review database constraints in migrations
4. Test with `nix-build -A nixosTests.firezone`
