{ config, lib, pkgs, ... }:

let
  inherit
    (lib)
    # TODO cleanup
    listToAttrs
    any
    attrValues
    attrNames
    concatMap
    concatMapStrings
    count
    flatten
    mapAttrsToList
    elem
    escapeShellArg
    escapeShellArgs
    filter
    flip
    subtractLists
    genAttrs
    getExe
    hasAttr
    hasInfix
    head
    literalExpression
    mapAttrs'
    mkBefore
    mkEnableOption
    mkIf
    mkOption
    nameValuePair
    optional
    optionals
    optionalString
    types
    unique
    ;

  format = pkgs.formats.json { };
  cfg = config.services.influxdb2;
  configFile = format.generate "config.json" cfg.settings;

  validPermissions = [
    "authorizations"
    "buckets"
    "dashboards"
    "orgs"
    "tasks"
    "telegrafs"
    "users"
    "variables"
    "secrets"
    "labels"
    "views"
    "documents"
    "notificationRules"
    "notificationEndpoints"
    "checks"
    "dbrp"
    "annotations"
    "sources"
    "scrapers"
    "notebooks"
    "remotes"
    "replications"
  ];

  # Determines whether at least one active api token is defined
  anyAuthDefined =
    flip any (attrValues cfg.provision.organizations)
    (o: o.present && flip any (attrValues o.auths)
    (a: a.present && a.tokenFile != null));

  provisionState = pkgs.writeText "provision_state.json" (builtins.toJSON {
    inherit (cfg.provision) organizations users;
  });

  provisioningScript = pkgs.writeShellScript "post-start-provision" ''
    set -euo pipefail
    export INFLUX_HOST="http://"${escapeShellArg (
      if ! hasAttr "http-bind-address" cfg.settings
        || hasInfix "0.0.0.0" cfg.settings.http-bind-address
      then "localhost:8086"
      else cfg.settings.http-bind-address
    )}

    # Wait for the influxdb server to come online
    count=0
    while ! influx ping &>/dev/null; do
      if [ "$count" -eq 300 ]; then
        echo "Tried for 30 seconds, giving up..."
        exit 1
      fi

      if ! kill -0 "$MAINPID"; then
        echo "Main server died, giving up..."
        exit 1
      fi

      sleep 0.1
      count=$((count++))
    done

    # Do the initial database setup. Pass /dev/null as configs-path to
    # avoid saving the token as the active config.
    if test -e "$STATE_DIRECTORY/.first_startup"; then
      influx setup \
        --configs-path /dev/null \
        --org ${escapeShellArg cfg.provision.initialSetup.organization} \
        --bucket ${escapeShellArg cfg.provision.initialSetup.bucket} \
        --username ${escapeShellArg cfg.provision.initialSetup.username} \
        --password "$(< "$CREDENTIALS_DIRECTORY/admin-password")" \
        --token "$(< "$CREDENTIALS_DIRECTORY/admin-token")" \
        --retention ${toString cfg.provision.initialSetup.retention}s \
        --force >/dev/null

      rm -f "$STATE_DIRECTORY/.first_startup"
    fi

    export INFLUX_TOKEN=$(< "$CREDENTIALS_DIRECTORY/admin-token")

    cat ${provisionState}
    #if ! ''${pkgs.influxdb2-provision} mapping.json; then
    #  if [[ "$?" != "75" ]]; then
    #    exit 1
    #  fi

    #  echo "Created new tokens, queueing service restart so we can manipulate secrets"
    #  touch "$STATE_DIRECTORY/.needs_restart"
    #fi
  '';

  restarterScript = pkgs.writeShellScript "post-start-restarter" ''
    set -euo pipefail
    if test -e "$STATE_DIRECTORY/.needs_restart"; then
      rm -f "$STATE_DIRECTORY/.needs_restart"
      systemctl restart influxdb2
    fi
  '';
in
{
  options = {
    services.influxdb2 = {
      enable = mkEnableOption (lib.mdDoc "the influxdb2 server");

      package = mkOption {
        default = pkgs.influxdb2-server;
        defaultText = literalExpression "pkgs.influxdb2";
        description = lib.mdDoc "influxdb2 derivation to use.";
        type = types.package;
      };

      settings = mkOption {
        default = { };
        description = lib.mdDoc ''configuration options for influxdb2, see <https://docs.influxdata.com/influxdb/v2.0/reference/config-options> for details.'';
        type = format.type;
      };

      provision = {
        enable = mkEnableOption "initial database setup and provisioning";

        initialSetup = {
          organization = mkOption {
            type = types.str;
            example = "main";
            description = "Primary organization name";
          };

          bucket = mkOption {
            type = types.str;
            example = "example";
            description = "Primary bucket name";
          };

          username = mkOption {
            type = types.str;
            default = "admin";
            description = "Primary username";
          };

          retention = mkOption {
			type = types.ints.unsigned;
            default = 0;
            description = "The duration in seconds for which the bucket will retain data (0 is infinite)."
          };

          passwordFile = mkOption {
            type = types.path;
            description = "Password for primary user. Don't use a file from the nix store!";
          };

          tokenFile = mkOption {
            type = types.path;
            description = "API Token to set for the admin user. Don't use a file from the nix store!";
          };
        };

        organizations = mkOption {
          description = "Organizations to provision.";
          example = literalExpression ''
            {
              myorg = {
                description = "My organization";
                buckets.mybucket = {
                  description = "My bucket";
                  retention = 31536000; # 1 year
                };
                auths.mytoken = {
                  readBuckets = ["mybucket"];
                  tokenFile = "/run/secrets/mytoken";
                };
              };
            }
          '';
          default = {};
          type = types.attrsOf (types.submodule (organizationSubmod: let
            org = organizationSubmod.config._module.args.name;
          in {
            options = {
              present = mkOption {
                description = "Whether to ensure that this organization is present or absent.";
                type = types.bool;
                default = true;
              };

              description = mkOption {
                description = "Optional description for the organization.";
                default = null;
                type = types.nullOr types.str;
              };

              buckets = mkOption {
                description = "Buckets to provision in this organization.";
                default = {};
                type = types.attrsOf (types.submodule (bucketSubmod: let
                  bucket = bucketSubmod.config._module.args.name;
                in {
                  options = {
                    present = mkOption {
                      description = "Whether to ensure that this bucket is present or absent.";
                      type = types.bool;
                      default = true;
                    };

                    description = mkOption {
                      description = "Optional description for the bucket.";
                      default = null;
                      type = types.nullOr types.str;
                    };

                    retention = mkOption {
                      type = types.ints.unsigned;
                      default = 0;
                      description = "The duration in seconds for which the bucket will retain data (0 is infinite).";
                    };
                  };
                }));
              };

              auths = mkOption {
                description = "API tokens to provision for the user in this organization.";
                default = {};
                type = types.attrsOf (types.submodule (authSubmod: let
                  auth = authSubmod.config._module.args.name;
                in {
                  options = {
                    id = mkOption {
                      description = "A unique identifier for this authentication token. Since influx doesn't store names for tokens, this will be hashed and appended to the description to identify the token.";
                      readOnly = true;
                      default = builtins.substring 0 32 (builtins.hashString "sha256" "${org}:${auth}");
                      defaultText = "<a hash derived from org and name>";
                      type = types.str;
                    };

                    present = mkOption {
                      description = "Whether to ensure that this user is present or absent.";
                      type = types.bool;
                      default = true;
                    };

                    description = mkOption {
                      description = ''
                        Optional description for the API token.
                        Note that the actual token will always be created with a descriptionregardless
                        of whether this is given or not. The name is always added plus a unique suffix
                        to later identify the token to track whether it has already been created.
                      '';
                      default = null;
                      type = types.nullOr types.str;
                    };

                    tokenFile = mkOption {
                      type = types.nullOr types.path;
                      default = null;
                      description = "The token value. If not given, influx will automatically generate one.";
                    };

                    operator = mkOption {
                      description = "Grants all permissions in all organizations.";
                      default = false;
                      type = types.bool;
                    };

                    allAccess = mkOption {
                      description = "Grants all permissions in the associated organization.";
                      default = false;
                      type = types.bool;
                    };

                    readPermissions = mkOption {
                      description = ''
                        The read permissions to include for this token. Access is usually granted only
                        for resources in the associated organization.

                        Available permissions are `authorizations`, `buckets`, `dashboards`,
                        `orgs`, `tasks`, `telegrafs`, `users`, `variables`, `secrets`, `labels`, `views`,
                        `documents`, `notificationRules`, `notificationEndpoints`, `checks`, `dbrp`,
                        `annotations`, `sources`, `scrapers`, `notebooks`, `remotes`, `replications`.

                        Refer to `influx auth create --help` for a full list with descriptions.

                        `buckets` grants read access to all associated buckets. Use `readBuckets` to define
                        more granular access permissions.
                      '';
                      default = [];
                      type = types.listOf (types.enum validPermissions);
                    };

                    writePermissions = mkOption {
                      description = ''
                        The read permissions to include for this token. Access is usually granted only
                        for resources in the associated organization.

                        Available permissions are `authorizations`, `buckets`, `dashboards`,
                        `orgs`, `tasks`, `telegrafs`, `users`, `variables`, `secrets`, `labels`, `views`,
                        `documents`, `notificationRules`, `notificationEndpoints`, `checks`, `dbrp`,
                        `annotations`, `sources`, `scrapers`, `notebooks`, `remotes`, `replications`.

                        Refer to `influx auth create --help` for a full list with descriptions.

                        `buckets` grants write access to all associated buckets. Use `writeBuckets` to define
                        more granular access permissions.
                      '';
                      default = [];
                      type = types.listOf (types.enum validPermissions);
                    };

                    readBuckets = mkOption {
                      description = "The organization's buckets which should be allowed to be read";
                      default = [];
                      type = types.listOf types.str;
                    };

                    writeBuckets = mkOption {
                      description = "The organization's buckets which should be allowed to be written";
                      default = [];
                      type = types.listOf types.str;
                    };
                  };
                }));
              };

              remotes = mkOption {
                description = "Remotes to provision in this organization.";
                default = {};
                type = types.attrsOf (types.submodule (remoteSubmod: let
                  remote = remoteSubmod.config._module.args.name;
                in {
                  options = {
                    present = mkOption {
                      description = "Whether to ensure that this remote is present or absent.";
                      type = types.bool;
                      default = true;
                    };

                    description = mkOption {
                      description = "Optional description for the remote.";
                      default = null;
                      type = types.nullOr types.str;
                    };

                    remoteUrl = mkOption {
                      description = "The url where the remote instance can be reached";
                      type = types.str;
                    };

                    remoteOrg = mkOption {
                      description = ''
                        Corresponding remote organization. If this is used instead of `remoteOrgId`,
                        the remote organization id must be queried first which means the provided remote
                        token must have the `read-orgs` flag.
                      '';
                      type = types.nullOr types.str;
                      default = null;
                    };

                    remoteOrgId = mkOption {
                      description = "Corresponding remote organization id.";
                      type = types.nullOr types.str;
                      default = null;
                    };

                    remoteTokenFile = mkOption {
                      type = types.path;
                      description = "API token used to authenticate with the remote.";
                    };

                    replications = mkOption {
                      description = ''
                        Replications to provision in this organization for this remote.
                        Beware that replication names must be unique in an organization.
                      '';
                      default = {};
                      type = types.attrsOf (types.submodule (replicationSubmod: let
                        replication = replicationSubmod.config._module.args.name;
                      in {
                        options = {
                          present = mkOption {
                            description = "Whether to ensure that this replication is present or absent.";
                            type = types.bool;
                            default = true;
                          };

                          localBucket = mkOption {
                            description = "The local bucket to replicate from.";
                            type = types.str;
                          };

                          remoteBucket = mkOption {
                            description = "The remte bucket to replicate to.";
                            type = types.str;
                          };
                        };
                      }));
                    };
                  };
                }));
              };
            };
          }));
        };

        users = mkOption {
          description = "Users to provision.";
          default = {};
          example = literalExpression ''
            {
              # admin = {}; /* The initialSetup.username will automatically be added. */
              myuser.passwordFile = "/run/secrets/myuser_password";
            }
          '';
          type = types.attrsOf (types.submodule (userSubmod: let
            user = userSubmod.config._module.args.name;
            org = userSubmod.config.org;
          in {
            options = {
              present = mkOption {
                description = "Whether to ensure that this user is present or absent.";
                type = types.bool;
                default = true;
              };

              passwordFile = mkOption {
                description = "Password for the user. If unset, the user will not be able to log in until a password is set by an operator! Don't use a file from the nix store!";
                default = null;
                type = types.nullOr types.path;
              };
            };
          }));
        };
      };
    };
  };

  config = mkIf cfg.enable {
    assertions =
      [
        {
          assertion = !(hasAttr "bolt-path" cfg.settings) && !(hasAttr "engine-path" cfg.settings);
          message = "services.influxdb2.config: bolt-path and engine-path should not be set as they are managed by systemd";
        }
      ]
      ++ flatten (flip mapAttrsToList cfg.provision.organizations (orgName: org:
        flip mapAttrsToList org.remotes (remoteName: remote:
          [
            {
              assertion = (remote.remoteOrgId == null) != (remote.remoteOrg == null);
              message = "influxdb2: provision.organizations.${orgName}.remotes.${remoteName}: Must specify exactly one of remoteOrgId or remoteOrg.";
            }
          ]
          ++ flip mapAttrsToList remote.replications (replicationName: replication:
            {
              assertion = org.buckets ? ${replication.localBucket};
              message = "influxdb2: provision.organizations.${orgName}.remotes.${remoteName}.replications.${replicationName}: Refers to unknown bucket '${replication.localBucket}'.";
            }
          )
        )
        ++ flip mapAttrsToList org.auths (authName: auth:
          [
            {
              assertion = 1 == count (x: x) [
                auth.operator
                auth.allAccess
                (auth.readPermissions != []
                  || auth.writePermissions != []
                  || auth.readBuckets != []
                  || auth.writeBuckets != [])
              ];
              message = "influxdb2: provision.organizations.${orgName}.auths.${authName}: The `operator` and `allAccess` options are mutually exclusive with each other and the granular permission settings.";
            }
            (let unknownBuckets = subtractLists (attrNames org.buckets) auth.readBuckets; in {
              assertion = unknownBuckets == [];
              message = "influxdb2: provision.organizations.${orgName}.auths.${authName}: Refers to invalid buckets in readBuckets: ${toString unknownBuckets}";
            })
            (let unknownBuckets = subtractLists (attrNames org.buckets) auth.writeBuckets; in {
              assertion = unknownBuckets == [];
              message = "influxdb2: provision.organizations.${orgName}.auths.${authName}: Refers to invalid buckets in writeBuckets: ${toString unknownBuckets}";
            })
          ]
        )
      ));

    services.influxdb2.provision = mkIf cfg.provision.enable {
      organizations.${cfg.provision.initialSetup.organization} = {
        buckets.${cfg.provision.initialSetup.bucket} = {
          inherit (cfg.provision.initialSetup) retention;
        };
      };
      users.${cfg.provision.initialSetup.username} = {
        inherit (cfg.provision.initialSetup) passwordFile;
      };
    };

    systemd.services.influxdb2 = {
      description = "InfluxDB is an open-source, distributed, time series database";
      documentation = [ "https://docs.influxdata.com/influxdb/" ];
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      environment = {
        INFLUXD_CONFIG_PATH = configFile;
        ZONEINFO = "${pkgs.tzdata}/share/zoneinfo";
      };
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/influxd --bolt-path \${STATE_DIRECTORY}/influxd.bolt --engine-path \${STATE_DIRECTORY}/engine";
        StateDirectory = "influxdb2";
        User = "influxdb2";
        Group = "influxdb2";
        CapabilityBoundingSet = "";
        SystemCallFilter = "@system-service";
        LimitNOFILE = 65536;
        KillMode = "control-group";
        Restart = "on-failure";
        LoadCredential = mkIf cfg.provision.enable [
          "admin-password:${cfg.provision.initialSetup.passwordFile}"
          "admin-token:${cfg.provision.initialSetup.tokenFile}"
        ];

        ExecStartPost = mkIf cfg.provision.enable (
          [provisioningScript] ++
          # Only the restarter runs with elevated privileges
          optional anyAuthDefined "+${restarterScript}"
        );
      };

      path = [
        pkgs.influxdb2-cli
        pkgs.jq
      ];

      # Mark if this is the first startup so postStart can do the initial setup
      preStart = let
        tokenPaths = listToAttrs (flatten (flip mapAttrsToList cfg.provision.organizations
          (_: org: flip mapAttrsToList org.auths
            (_: token: nameValuePair token.id token.tokenFile))));
        tokenMappings = pkgs.writeText "token_mappings.json" (builtins.toJSON tokenPaths);
      in mkIf cfg.provision.enable ''
        cat ${ tokenMappings /* TODO aaaaaa */}
        if ! test -e "$STATE_DIRECTORY/influxd.bolt"; then
          touch "$STATE_DIRECTORY/.first_startup"
        else
          # Manipulate provisioned api tokens if necessary
          ${getExe pkgs.influxdb2-token-manipulator} "$STATE_DIRECTORY/influxd.bolt" ${tokenMappings}
        fi
      '';
    };

    users.extraUsers.influxdb2 = {
      isSystemUser = true;
      group = "influxdb2";
    };

    users.extraGroups.influxdb2 = {};
  };

  meta.maintainers = with lib.maintainers; [ nickcao oddlama ];
}
