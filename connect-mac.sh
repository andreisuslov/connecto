#!/bin/bash

# Cloudflare Access Service Token Credentials
export CF_ACCESS_CLIENT_ID="e60f3ee8852d05023b251ff55ba66621.access"
export CF_ACCESS_CLIENT_SECRET="e358276618537c16b71b75727b268fe047dc34bcc091dc9de55f8489e3bf057c"

# SSH through Cloudflare Access
# Option 1: Using cloudflared access ssh (recommended)
cloudflared access ssh --hostname ssh.andreisuslov.com

# Alternative: If you need to specify a user
# cloudflared access ssh --hostname ssh.andreisuslov.com -l <username>
