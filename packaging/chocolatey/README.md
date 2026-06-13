# Chocolatey package

This package is packed and pushed manually; no CI job publishes it.

`tools/chocolateyInstall.ps1` derives the download URL from the package
version (`$env:ChocolateyPackageVersion`), so the URL always matches the
`<version>` in `connecto.nuspec`. The zip checksum cannot be derived at
install time and must be updated by hand for every release — the script
ships with a placeholder that fails checksum validation loudly if you
forget.

## Publishing a new version

Prerequisite: the GitHub release `v$VERSION` exists and includes the
`connecto-windows-x86_64.zip` and `.zip.sha256` assets (produced by
`.github/workflows/release.yml`).

From `packaging/chocolatey/`:

```bash
VERSION=0.5.1   # the released version

# 1. Stamp the version into the nuspec
sed -i.bak -E "s|<version>[^<]*</version>|<version>${VERSION}</version>|" connecto.nuspec \
  && rm connecto.nuspec.bak

# 2. Stamp the published zip's SHA-256 into the install script
CHECKSUM=$(curl -fsSL "https://github.com/andreisuslov/connecto/releases/download/v${VERSION}/connecto-windows-x86_64.zip.sha256" | awk '{print $1}')
sed -i.bak -E "s/(checksum      = ')[^']*(')/\1${CHECKSUM}\2/" tools/chocolateyInstall.ps1 \
  && rm tools/chocolateyInstall.ps1.bak
```

Then pack and push:

```powershell
choco pack
choco push connecto.$env:VERSION.nupkg --source https://push.chocolatey.org/
```

Note: on tag builds, `release.yml` also stamps the nuspec version as a
build-time guard, but that change is runner-local; the repository copy is
updated by step 1 above.
