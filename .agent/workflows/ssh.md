---
description: how to connect via SSH to the VPN server and run commands
---

# SSH Server Access Skill

The VPN server is accessible at:

- **Host**: `89.169.54.87`
- **User**: `root`
- **Auth**: SSH key (in `~/.ssh/lowkey_deploy` on this machine) or password `pvSr02KmQYYc`

## Connecting via SSH

Use `plink` (PuTTY) or the built-in `ssh` command:

```powershell
# Connect interactively
ssh root@89.169.54.87

# Run a single command and get output
ssh root@89.169.54.87 "command here"

# Run multiple commands
ssh root@89.169.54.87 @"
command1
command2
command3
"@
```

## Using run_command to execute remote commands

```powershell
# Example: check running docker containers
ssh -o StrictHostKeyChecking=no -o BatchMode=yes root@89.169.54.87 "docker ps"

# Example: view server logs
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "docker compose -f /opt/lowkey/docker-compose.yml logs --tail=50"

# Example: check if web is running
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "curl -s http://localhost:3000 | head -20"
```

## App deployment location on server

The app is deployed to `/opt/lowkey/`.

```powershell
# View deployment directory
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "ls -la /opt/lowkey/"

# View running services
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "docker compose -f /opt/lowkey/docker-compose.yml ps"

# View logs
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "docker compose -f /opt/lowkey/docker-compose.yml logs --tail=100"

# Restart services
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "cd /opt/lowkey && docker compose up -d --build"
```

## Accepting host key (first time)

First time connecting, accept the host key:

```powershell
ssh -o StrictHostKeyChecking=no root@89.169.54.87 "echo connected"
```

## Notes

- The server runs Docker Compose with: `postgres`, `vpn-server`, `web` services
- API is on port 8080, web on port 3000
- VPN tunnel on UDP 51820, SOCKS5 proxy on 8388
