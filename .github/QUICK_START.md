# Quick Start: Snap Store Releases

## One-Time Setup (5 minutes)

### 1. Export Credentials
```bash
snapcraft export-login --snaps=llamafile-builder --channels=stable snapcraft-credentials.txt
```

### 2. Add to GitHub Secrets
1. Go to: **Settings** → **Secrets and variables** → **Actions**
2. Click: **New repository secret**
3. Name: `SNAPCRAFT_STORE_CREDENTIALS`
4. Value: Paste contents of `snapcraft-credentials.txt`
5. Delete local file: `rm snapcraft-credentials.txt`

## Creating Releases

### Release a New Version
```bash
# 1. Update version in src-tauri/Cargo.toml
# 2. Commit changes
git add src-tauri/Cargo.toml
git commit -m "chore: bump version to 0.2.0"

# 3. Create and push tag
git tag v0.2.0
git push origin v0.2.0
```

**That's it!** The GitHub Action will:
- ✅ Build for amd64, arm64, and armhf (in parallel)
- ✅ Publish to Snap Store stable channel
- ✅ Upload build artifacts to GitHub
- ⏱️ Complete in ~25-35 minutes

### Monitor Progress
- **Actions tab:** See build logs and status
- **Snap Store:** Check https://snapcraft.io/llamafile-builder/releases
- **Install:** `sudo snap install llamafile-builder`

## Troubleshooting

**Authentication failed?**
```bash
# Regenerate credentials and update GitHub Secret
snapcraft export-login --snaps=llamafile-builder --channels=stable snapcraft-credentials.txt
```

**Build failed?**
- Check the Actions logs for specific errors
- Download artifacts from the workflow run for local debugging
- Re-run the workflow (transient failures are common with LXD)

## Full Documentation

See [SNAP_RELEASE_SETUP.md](SNAP_RELEASE_SETUP.md) for complete details.
