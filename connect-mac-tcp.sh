#!/bin/bash

# Cloudflare Access Service Token Credentials
export CF_ACCESS_CLIENT_ID="e60f3ee8852d05023b251ff55ba66621.access"
export CF_ACCESS_CLIENT_SECRET="e358276618537c16b71b75727b268fe047dc34bcc091dc9de55f8489e3bf057c"

# TCP connection through Cloudflare Access
# This creates a local tunnel to the remote TCP service
cloudflared access tcp --hostname tcp.andreisuslov.com --url localhost:2222

# Then in another terminal, connect with:
# ssh -p 2222 <username>@localhost
