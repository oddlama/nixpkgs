import ./make-test-python.nix ({ pkgs, ...} : {
  name = "influxdb2";
  meta = with pkgs.lib.maintainers; {
    maintainers = [ offline ];
  };

  nodes.machine = { lib, ... }: {
    environment.systemPackages = [ pkgs.influxdb2-cli ];
    # Make sure that the service is restarted immediately if tokens need to be rewritten
    # without relying on any Restart=on-failure behavior
    systemd.services.influxdb2.serviceConfig.RestartSec = 6000;
    services.influxdb2.enable = true;
    services.influxdb2.provision = {
      enable = true;
      initialSetup = {
        organization = "default";
        bucket = "default";
        passwordFile = pkgs.writeText "admin-pw" "ExAmPl3PA55W0rD";
        tokenFile = pkgs.writeText "admin-token" "verysecureadmintoken";
      };
      organizations.delorg = {
        buckets.delbucket = {};
        apiTokens.deltoken = {
          description = "del auth token";
          readBuckets = ["delbucket"];
          writeBuckets = ["delbucket"];
        };
        remotes.delremote = {
          remoteUrl = "http://localhost:8087";
          remoteOrgId = "a1b2c3d4a1b2c3d4";
          remoteTokenFile = pkgs.writeText "remote-token" "verysecureremotetoken";
          replications.delreplication = {
            localBucket = "delbucket";
            remoteBucket = "coolremotebucket";
          };
        };
      };
      users.deluser.passwordFile = pkgs.writeText "tmp-pw" "abcgoiuhaoga";
    };

    specialisation.withModifications.configuration = { ... }: {
      services.influxdb2.provision = {
        organizations.delorg.present = false;
        # This implies:
        #organizations.delorg.buckets.delbucket.present = false;
        #organizations.delorg.apiTokens.deltoken.present = false;
        #organizations.delorg.remotes.delremote.present = false;
        #organizations.delorg.remotes.delremote.replications.delreplication.present = false;
        users.deluser.present = false;

        organizations.myorg = {
          description = "Myorg description";
          buckets.mybucket = {
            description = "Mybucket description";
          };
          apiTokens.mytoken = {
            operator = true;
            description = "operator token";
            tokenFile = pkgs.writeText "tmp-tok" "someusertoken";
          };
        };
        users.myuser.passwordFile = pkgs.writeText "tmp-pw" "abcgoiuhaoga";
      };
    };
  };

  testScript = { nodes, ... }:
    let
      specialisations = "${nodes.machine.system.build.toplevel}/specialisation";
      tokenArg = "--token verysecureadmintoken";
    in ''
      machine.wait_for_unit("influxdb2.service")

      machine.fail("curl --fail -X POST 'http://localhost:8086/api/v2/signin' -u admin:wrongpassword")
      machine.succeed("curl --fail -X POST 'http://localhost:8086/api/v2/signin' -u admin:ExAmPl3PA55W0rD")

      out = machine.succeed("influx org list ${tokenArg}")
      assert "default" in out
      assert "myorg" not in out
      assert "delorg" in out

      out = machine.succeed("influx bucket list ${tokenArg} --org default")
      assert "default" in out

      machine.fail("influx bucket list ${tokenArg} --org myorg")

      out = machine.succeed("influx bucket list ${tokenArg} --org delorg")
      assert "delbucket" in out

      out = machine.succeed("influx user list ${tokenArg}")
      assert "admin" in out
      assert "myuser" not in out
      assert "deluser" in out

      out = machine.succeed("influx remote list ${tokenArg} --org delorg")
      assert "delremote" in out

      out = machine.succeed("influx replication list ${tokenArg} --org delorg")
      assert "delreplication" in out

      out = machine.succeed("influx auth list ${tokenArg}")
      assert "operator token" not in out
      assert "del auth token" in out

      with subtest("withModifications"):
        machine.succeed('${specialisations}/withModifications/bin/switch-to-configuration test')
        machine.wait_for_unit("influxdb2.service")

        out = machine.succeed("influx org list ${tokenArg}")
        assert "default" in out
        assert "myorg" in out
        assert "delorg" not in out

        out = machine.succeed("influx bucket list ${tokenArg} --org myorg")
        assert "mybucket" in out

        machine.fail("influx bucket list ${tokenArg} --org delorg")

        out = machine.succeed("influx user list ${tokenArg}")
        assert "admin" in out
        assert "myuser" in out
        assert "deluser" not in out

        machine.fail("influx remote list ${tokenArg} --org delorg")
        machine.fail("influx replication list ${tokenArg} --org delorg")

        out = machine.succeed("influx auth list ${tokenArg}")
        assert "operator token" in out
        assert "del auth token" not in out

        # Make sure the user token is also usable
        machine.succeed("influx auth list --token someusertoken")
    '';
})
