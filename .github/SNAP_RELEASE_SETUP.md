# Snap Store Release Setup Guide

This guide explains how to configure GitHub Actions to automatically build and publish Snap packages to the Snap Store when you push version tags.

## Prerequisites

Before the GitHub Action can publish to the Snap Store, you need to:

1. **Have a Snap Store developer account** at [snapcraft.io](https://snapcraft.io/)
2. **Register the snap name** (if not already done)
3. **Generate and configure credentials** in GitHub Secrets

## Step-by-Step Setup

### 1. Register Your Snap Name (If Not Already Done)

```bash
snapcraft register llamafile-builder
```

This reserves the name "llamafile-builder" on the Snap Store.

### 2. Export Snap Store Credentials

Generate a credentials file that GitHub Actions will use to authenticate with the Snap Store:

```bash
snapcraft export-login --snaps=llamafile-builder --channels=stable snapcraft-credentials.txt
```

This will:
- Prompt you to log in to your Snap Store account (if not already logged in)
- Create a file `snapcraft-credentials.txt` with your credentials
- Scope the credentials to only the `llamafile-builder` snap and `stable` channel

**Important Security Notes:**
- This file contains sensitive credentials
- Never commit this file to git
- Delete it immediately after adding to GitHub Secrets

### 3. Add Credentials to GitHub Secrets

1. **Navigate to your repository on GitHub**
   - Go to: `Settings` → `Secrets and variables` → `Actions`

2. **Create a new repository secret**
   - Click the "New repository secret" button
   - Name: `SNAPCRAFT_STORE_CREDENTIALS`
   - Value: Paste the **entire contents** of `snapcraft-credentials.txt`
   - Click "Add secret"

3. **Delete the credentials file from your local system**
   ```bash
   rm snapcraft-credentials.txt
   ```

### 4. Verify Setup

To test that everything is configured correctly:

1. **Create a version tag and push it:**
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

2. **Monitor the GitHub Action:**
   - Go to the "Actions" tab in your GitHub repository
   - You should see a new workflow run for "Build and Publish Snap"
   - The workflow will build for amd64, arm64, and armhf architectures in parallel

3. **Check the Snap Store:**
   - Visit [snapcraft.io/llamafile-builder/releases](https://snapcraft.io/llamafile-builder/releases)
   - Your new version should appear in the stable channel after the build completes

## Usage

### Releasing a New Version

1. **Update version in `src-tauri/Cargo.toml`:**
   ```toml
   [package]
   version = "0.2.0"
   ```

2. **Commit the version change:**
   ```bash
   git add src-tauri/Cargo.toml
   git commit -m "chore: bump version to 0.2.0"
   git push
   ```

3. **Create and push a version tag:**
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. **Wait for builds to complete:**
   - Check the "Actions" tab on GitHub
   - All three architectures (amd64, arm64, armhf) will build in parallel
   - Build time: ~25-35 minutes total
   - Each snap will be automatically uploaded to the Snap Store stable channel

5. **Verify the release:**
   ```bash
   snap info llamafile-builder
   ```

### Tag Naming Convention

The workflow triggers on tags matching the pattern `v*`, for example:
- `v0.1.0` - Initial release
- `v1.0.0` - Major release
- `v1.2.3` - Standard semantic version
- `v2.0.0-beta.1` - Pre-release (still goes to stable channel)

**Note:** All tags automatically publish to the **stable** channel. If you need beta/candidate testing, push tags manually or modify the workflow.

## Troubleshooting

### Build Failures

If a build fails:

1. **Check the Actions logs:**
   - Go to Actions → Click on the failed workflow run
   - Expand the failed step to see error details

2. **Common issues:**
   - **LXD setup errors:** Usually transient, re-run the workflow
   - **Build timeouts:** Increase `timeout-minutes` in the workflow
   - **Architecture-specific failures:** Check if all required dependencies are available for that arch

3. **Download build artifacts:**
   - Even if publishing fails, snap files are uploaded as GitHub artifacts
   - You can download them for local debugging
   - Artifacts are kept for 30 days

### Authentication Failures

If you see "Authentication failed" errors:

1. **Regenerate credentials:**
   ```bash
   snapcraft export-login --snaps=llamafile-builder --channels=stable snapcraft-credentials.txt
   ```

2. **Update GitHub Secret:**
   - Go to Settings → Secrets → Actions
   - Edit `SNAPCRAFT_STORE_CREDENTIALS`
   - Paste new credentials
   - Save changes

3. **Verify credentials locally:**
   ```bash
   cat snapcraft-credentials.txt | snapcraft login --with -
   snapcraft whoami
   snapcraft logout
   ```

### Manual Publishing

If you need to manually publish a snap:

1. **Download the artifact from GitHub Actions**
2. **Upload manually:**
   ```bash
   snapcraft login
   snapcraft upload --release=stable llamafile-builder_*.snap
   snapcraft logout
   ```

## Architecture Support

The workflow builds for three architectures:

- **amd64** (x86_64): Most desktop/laptop computers
- **arm64** (aarch64): ARM servers, Raspberry Pi 4/5, Apple Silicon (via virtualization)
- **armhf** (armv7): Older ARM devices, Raspberry Pi 2/3

Each architecture is built independently and uploaded to the Snap Store. Users will automatically receive the correct version for their system.

## Security Best Practices

1. **Never commit credentials:**
   - The `.gitignore` should exclude `*.txt` files with credentials
   - Always use GitHub Secrets for sensitive data

2. **Limit credential scope:**
   - Use `--snaps=` and `--channels=` flags when exporting credentials
   - This limits what the credentials can access

3. **Rotate credentials regularly:**
   - Generate new credentials every few months
   - Update GitHub Secrets with new credentials

4. **Monitor releases:**
   - Check Snap Store releases regularly
   - Ensure no unauthorized uploads

## Additional Resources

- [Snapcraft Documentation](https://snapcraft.io/docs)
- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Snap Store Dashboard](https://snapcraft.io/snaps)
- [llamafile Builder Snap Page](https://snapcraft.io/llamafile-builder)

## Support

If you encounter issues:
1. Check the GitHub Actions logs first
2. Review this guide for troubleshooting steps
3. Open an issue in the repository with relevant error logs
