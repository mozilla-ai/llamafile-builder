// llamafile Builder - Main Application
// Tauri frontend with state management and backend command invocation

// ============================================================================
// State Management
// ============================================================================

const state = {
  currentStep: 1,
  maxVisitedStep: 1,

  // Step 1: Version (grouped)
  versionGroups: [],
  selectedVersionGroup: null,
  selectedRelease: null,
  expandedGroups: {},
  llamafileSource: 'registry',   // 'registry' or 'custom'
  customLlamafilePath: null,
  resolvedLlamafilePath: null,

  // Step 2: Model
  models: [],
  filteredModels: [],
  selectedModel: null,
  modelSource: 'registry',       // 'registry' or 'custom'
  modelFileSelections: {},        // keyed by file role: { source, quantizationIdx, customPath }
  expandedModelFiles: {},         // keyed by file role: bool
  modelDownloadDir: '',
  customModelFiles: [],           // array of { role, path }
  searchQuery: '',
  activeFilter: 'all',

  // Step 3: Configuration (schema-driven)
  configSchema: [],
  configValues: {},

  // Step 4: Compute
  computeMode: 'gpu',
  gpuOptions: {
    cuda: true,
    rocm: true,
    metal: true,
  },

  // Step 5: Agent
  agentEnabled: false,
  agentTools: [],
  systemPrompt: `# Agent Instructions

You are a helpful coding assistant with access to local tools.

## Guidelines

- Always explain before executing
- Ask for confirmation on destructive operations
- Prefer minimal, targeted changes`,

  // Step 6: Build
  outputPath: '',
  argsContent: '', // Generated .args file content (editable by user)
  buildProgress: null,
  isBuilding: false,
  buildLog: [],
  downloadProgress: null, // { filename, bytes_downloaded, bytes_total, percentage }
  hfTokenPromptVisible: false,
  hfTokenPendingRetry: null, // { url, destPath, sha256 } - retry after token entry
  fileExistsPrompt: null, // { filename, path, resolve } - prompt when file exists
  cancelConfirmVisible: false, // show cancel confirmation modal
  currentDownloadPath: null, // path of file currently being downloaded (for cleanup)
  shaVerifyPrompt: null, // { role, path, filename, expectedSha, resolve } - prompt for custom file SHA verification
  shaVerifyProgress: null, // { filename } - shown while calculating SHA
};

const STEPS = [
  { id: 1, name: 'Version', icon: '1' },
  { id: 2, name: 'Model', icon: '2' },
  { id: 3, name: 'Configuration', icon: '3' },
  { id: 4, name: 'Compute', icon: '4' },
  { id: 5, name: 'Agent', icon: '5' },
  { id: 6, name: 'Build', icon: '6' },
];

// ============================================================================
// Tauri Command Helpers
// ============================================================================

const invoke = window.__TAURI__.core.invoke;

/**
 * Sync JS state from Rust BuildState
 * Maps Rust camelCase fields to JS state fields
 */
function syncFromBuildState(buildState) {
  // Model selection
  state.modelSource = buildState.modelSource;
  state.modelFileSelections = buildState.modelFileSelections || {};
  state.customModelFiles = buildState.customModelFiles || [];

  // If model source is registry and we have a model ID, find the model
  if (buildState.modelSource === 'registry' && buildState.selectedModelId) {
    state.selectedModel = state.models.find(m => m.id === buildState.selectedModelId) || null;
  } else if (buildState.modelSource === 'custom') {
    state.selectedModel = null;
  }

  // Llamafile selection
  state.llamafileSource = buildState.llamafileSource;
  state.customLlamafilePath = buildState.customLlamafilePath;
  state.resolvedLlamafilePath = buildState.resolvedLlamafilePath;

  // If llamafile source is registry and we have version info, find the group/release
  if (buildState.llamafileSource === 'registry' && buildState.selectedVersionGroup) {
    state.selectedVersionGroup = state.versionGroups.find(g => g.name === buildState.selectedVersionGroup) || null;
    if (state.selectedVersionGroup && buildState.selectedReleaseVersion) {
      state.selectedRelease = state.selectedVersionGroup.releases.find(r => r.version === buildState.selectedReleaseVersion) || null;
    }
  } else if (buildState.llamafileSource === 'custom') {
    state.selectedVersionGroup = null;
    state.selectedRelease = null;
  }

  // Config - merge with existing values (don't overwrite defaults from schema)
  if (buildState.configValues) {
    state.configValues = { ...state.configValues, ...buildState.configValues };
  }
  // Output path
  state.outputPath = buildState.outputPath || state.outputPath || '';
}

const listen = window.__TAURI__.event.listen;

// ============================================================================
// Sync JS state from Rust BuildState
// ============================================================================

function getMockAgentTools() {
  return [
    { id: 'shell', name: 'Shell Exec', description: 'Execute shell commands', icon: '\ud83d\udcbb', enabled: false },
    { id: 'file_io', name: 'File I/O', description: 'Read and write files', icon: '\ud83d\udcc1', enabled: false },
    { id: 'web_fetch', name: 'Web Fetch', description: 'Fetch URLs and web content', icon: '\ud83c\udf10', enabled: false },
    { id: 'python', name: 'Python REPL', description: 'Execute Python code', icon: '\ud83d\udc0d', enabled: false },
    { id: 'sqlite', name: 'SQLite', description: 'Query SQLite databases', icon: '\ud83d\uddc4\ufe0f', enabled: false },
    { id: 'data_analysis', name: 'Data Analysis', description: 'Analyze and visualize data', icon: '\ud83d\udcca', enabled: false },
  ];
}

// ============================================================================
// Initialization
// ============================================================================

async function init() {
  try {
    // Load version groups and models first (needed for sync)
    state.versionGroups = await invoke('get_llamafile_versions');
    state.models = await invoke('get_models');
    state.filteredModels = [...state.models];

    // Load model download directory
    try {
      state.modelDownloadDir = await invoke('get_model_cache_dir') || '';
    } catch (e) {
      state.modelDownloadDir = '~/.llamafile/models';
    }

    // Load config schema and initialize defaults
    state.configSchema = await invoke('get_config_schema');
    state.configValues = {};
    initConfigDefaults(state.configSchema);

    function initConfigDefaults(items) {
      for (const item of items) {
        if (item.key && item.default !== undefined) {
          state.configValues[item.key] = item.default;
        }
        // Recurse into nested items
        if (item.items) {
          initConfigDefaults(item.items);
        }
      }
    }

    // Select first version group (calls Rust to update build state)
    if (state.versionGroups.length > 0) {
      await selectVersionGroup(state.versionGroups[0].name);
    }

    // Select first model (calls Rust to update build state)
    if (state.models.length > 0) {
      await selectModel(state.models[0].id);
    }

    state.agentTools = getMockAgentTools();

    // Output path is now set by Rust via selectModel
    if (!state.outputPath && state.selectedModel) {
      state.outputPath = await invoke('get_default_output_path', {
        ggufFilename: getSelectedGgufFilename(),
        modelName: state.selectedModel.name
      });
    }
  } catch (err) {
    console.error('Failed to load initial data:', err);
  }

  listen('build-progress', (event) => {
    state.buildProgress = event.payload;
    renderBuildProgress();
  });

  listen('build-log', (event) => {
    state.buildLog.push(event.payload);
    renderBuildLog();
  });

  listen('download-progress', (event) => {
    const wasNull = state.downloadProgress === null;
    state.downloadProgress = event.payload;
    // Update build progress with download percentage
    if (state.buildProgress && state.isBuilding) {
      state.buildProgress.progress_pct = event.payload.percentage;
      state.buildProgress.message = `Downloading ${event.payload.filename}...`;
    }
    // First progress event - need full re-render to create the progress bar element
    if (wasNull) {
      renderCurrentStep();
    } else {
      renderDownloadProgress();
    }
  });

  // Open external links in system browser
  document.addEventListener('click', async (e) => {
    const link = e.target.closest('a[href]');
    if (link && link.href && link.href.startsWith('http')) {
      e.preventDefault();
      try {
        await invoke('open_url', { url: link.href });
      } catch (err) {
        console.error('Failed to open URL:', err);
        window.open(link.href, '_blank');
      }
    }
  });

  // Event delegation system - handles all clicks via data-action attributes
  document.addEventListener('click', handleGlobalClick, true);
  document.addEventListener('input', handleGlobalInput, false);
  document.addEventListener('change', handleGlobalChange, false);
  document.addEventListener('keydown', handleGlobalKeydown, false);

  renderSidebar();
  renderCurrentStep();
}

// ============================================================================
// Rendering Functions
// ============================================================================

function renderSidebar() {
  const stepList = document.getElementById('step-list');
  stepList.innerHTML = STEPS.map(step => {
    const isActive = step.id === state.currentStep;
    const isCompleted = step.id < state.currentStep;
    const isDisabled = step.id > state.maxVisitedStep;

    let classes = 'step-item';
    if (isActive) classes += ' active';
    if (isCompleted) classes += ' completed';
    if (isDisabled) classes += ' disabled';

    return `
      <li class="${classes}" data-action="goToStep" data-step="${step.id}">
        <div class="step-num">${isCompleted ? '\u2713' : step.icon}</div>
        ${step.name}
      </li>
    `;
  }).join('');
}

function renderCurrentStep() {
  const content = document.getElementById('step-content');

  // Preserve scroll position across re-renders
  const prevBody = content.querySelector('.content-body');
  const scrollTop = prevBody ? prevBody.scrollTop : 0;

  switch (state.currentStep) {
    case 1: content.innerHTML = renderVersionStep(); break;
    case 2: content.innerHTML = renderModelStep(); break;
    case 3: content.innerHTML = renderConfigStep(); break;
    case 4: content.innerHTML = renderComputeStep(); break;
    case 5: content.innerHTML = renderAgentStep(); break;
    case 6: content.innerHTML = renderBuildStep(); break;
  }

  const body = content.querySelector('.content-body');
  if (body) {
    body.classList.add('fade-in');
    body.scrollTop = scrollTop;
  }

  // Render modals to separate container (avoids re-render flicker)
  renderModals();
}

function renderModals() {
  const container = document.getElementById('modal-container');
  if (!container) return;

  let html = '';

  // HF Token prompt modal
  if (state.hfTokenPromptVisible) {
    html += `
      <div class="hf-token-modal">
        <div class="hf-token-content">
          <div class="hf-token-title">Authentication Required</div>
          <div class="hf-token-desc">This model requires a Hugging Face token. You can get one at <a href="https://huggingface.co/settings/tokens" target="_blank">huggingface.co/settings/tokens</a></div>
          <input type="password" class="hf-token-input" id="hf-token-input" placeholder="hf_..." onkeydown="if(event.key==='Enter') submitHfToken(this.value)">
          <div class="hf-token-buttons">
            <button class="btn btn-secondary" onclick="cancelHfTokenPrompt()">Cancel</button>
            <button class="btn btn-primary" onclick="submitHfToken(document.getElementById('hf-token-input').value)">Save &amp; Retry</button>
          </div>
        </div>
      </div>
    `;
  }

  // File exists prompt modal
  if (state.fileExistsPrompt) {
    html += `
      <div class="hf-token-modal">
        <div class="hf-token-content">
          ${state.fileExistsPrompt.isPartial ? `
            <div class="hf-token-title">Incomplete Download Found</div>
            <div class="hf-token-desc">
              <code>${state.fileExistsPrompt.filename}</code> appears to be a partial download.<br>
              <span style="color: var(--text-muted)">
                ${formatBytes(state.fileExistsPrompt.currentSize)} of ${formatBytes(state.fileExistsPrompt.expectedSize)} downloaded
              </span>
            </div>
            <div class="hf-token-buttons" style="margin-top: 16px;">
              <button class="btn btn-primary" onclick="handleFileExistsChoice('resume')">Resume Download</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('download')">Start Fresh</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('cancel')" style="margin-left: auto;">Cancel</button>
            </div>
          ` : state.fileExistsPrompt.checksumMismatch ? `
            <div class="hf-token-title">Checksum Mismatch</div>
            <div class="hf-token-desc">
              <code>${state.fileExistsPrompt.filename}</code> exists but the checksum doesn't match.<br>
              <span style="color: var(--accent)">The file may be corrupted or from a different version.</span>
            </div>
            <div class="hf-token-buttons" style="margin-top: 16px;">
              <button class="btn btn-primary" onclick="handleFileExistsChoice('download')">Re-download</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('skip')">Use Anyway</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('cancel')" style="margin-left: auto;">Cancel</button>
            </div>
          ` : state.fileExistsPrompt.askVerifyChecksum ? `
            <div class="hf-token-title">Verify File Integrity?</div>
            <div class="hf-token-desc">
              <code>${state.fileExistsPrompt.filename}</code> exists and the file size matches
              (${formatBytes(state.fileExistsPrompt.expectedSize)}).<br>
              <span style="color: var(--text-muted)">Would you like to verify the SHA256 checksum? This may take a moment for large files.</span>
            </div>
            <div class="hf-token-buttons" style="margin-top: 16px;">
              <button class="btn btn-primary" onclick="handleFileExistsChoice('verify')">Verify SHA256</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('skip')">Skip Verification</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('download')">Re-download</button>
            </div>
          ` : `
            <div class="hf-token-title">File Already Exists</div>
            <div class="hf-token-desc">
              <code>${state.fileExistsPrompt.filename}</code> already exists in the cache.${
                state.fileExistsPrompt.isComplete
                  ? (state.fileExistsPrompt.checksumVerified
                      ? '<br><span style="color: var(--green)">File is complete and checksum verified.</span>'
                      : '<br><span style="color: var(--text-secondary)">File size matches (no checksum available to verify).</span>')
                  : ''
              }
            </div>
            <div class="hf-token-buttons" style="margin-top: 16px;">
              <button class="btn btn-primary" onclick="handleFileExistsChoice('skip')">Use Existing</button>
              <button class="btn btn-secondary" onclick="handleFileExistsChoice('download')">Re-download</button>
            </div>
          `}
        </div>
      </div>
    `;
  }

  // Cancel confirm modal
  if (state.cancelConfirmVisible) {
    html += `
      <div class="hf-token-modal">
        <div class="hf-token-content">
          <div class="hf-token-title">Cancel Build?</div>
          <div class="hf-token-desc">
            Are you sure you want to cancel the build?${state.downloadProgress ? '<br><br><span style="color: var(--accent)">The current download will be stopped and the partial file will be deleted.</span>' : ''}
          </div>
          <div class="hf-token-buttons" style="margin-top: 16px;">
            <button class="btn btn-secondary" onclick="dismissCancelConfirm()">Continue Build</button>
            <button class="btn btn-primary" style="background: var(--red);" onclick="confirmCancelBuild()">Cancel Build</button>
          </div>
        </div>
      </div>
    `;
  }

  container.innerHTML = html;
}

// ============================================================================
// Step 1: Version Selection (Grouped)
// ============================================================================

function renderSimpleMarkdown(text) {
  if (!text) return '';
  return text
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" title="$2">$1</a>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\*([^*]+)\*/g, '<em>$1</em>');
}

function renderVersionStep() {
  const badgeClass = (tag) => ({
    latest: 'badge-latest',
    stable: 'badge-stable',
    legacy: 'badge-legacy',
  }[tag] || 'badge-legacy');

  const iconBg = (tag) => ({
    latest: 'background: var(--green-dim); color: var(--green);',
    stable: 'background: var(--blue-dim); color: var(--blue);',
    legacy: 'background: var(--purple-dim); color: var(--purple);',
  }[tag] || '');

  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">llamafile Version</div>
        <div class="content-subtitle">Select which version of the llamafile runtime to bundle.</div>
      </div>
      <div class="content-body">
        ${state.versionGroups.map(g => {
          const isGroupSelected = state.llamafileSource === 'registry' && state.selectedVersionGroup?.name === g.name;
          const isExpanded = !!state.expandedGroups[g.name];
          const hasMultiple = g.releases.length > 1;
          const shortVersion = g.name.replace('llamafile ', '');

          return `
            <div class="version-group ${isGroupSelected ? 'selected' : ''}">
              <div class="selection-card ${isGroupSelected ? 'selected' : ''}"
                   data-action="selectVersionGroup" data-group-name="${g.name}">
                <div class="radio-dot"></div>
                <div class="card-icon" style="${iconBg(g.tag)}">
                  <span style="font-family: 'JetBrains Mono'; font-size: 11px; font-weight: 700;">${shortVersion}</span>
                </div>
                <div class="card-info">
                  <div class="card-title">${g.name}</div>
                  <div class="card-desc">${g.description ? renderSimpleMarkdown(g.description) : ''}</div>
                </div>
                <div class="card-badge ${badgeClass(g.tag)}">${g.tag}</div>
                ${hasMultiple ? `
                  <div class="expand-toggle" data-action="toggleVersionGroup" data-group-name="${g.name}" data-stop-propagation="true">
                    <span class="expand-icon ${isExpanded ? 'expanded' : ''}">&#9654;</span>
                    <span class="expand-count">${g.releases.length}</span>
                  </div>
                ` : ''}
              </div>
              ${isExpanded ? `
                <div class="version-releases">
                  ${g.releases.map(r => {
                    const isSelected = state.selectedRelease?.version === r.version;
                    return `
                      <div class="release-row ${isSelected ? 'selected' : ''}"
                           data-action="selectSpecificRelease" data-group-name="${g.name}" data-version="${r.version}">
                        <div class="release-radio"></div>
                        <div class="release-info">
                          <span class="release-version">${r.version}</span>
                          ${r.description ? `<span class="release-desc">${renderSimpleMarkdown(r.description)}</span>` : ''}
                        </div>
                        <span class="release-date">${r.release_date}</span>
                      </div>
                    `;
                  }).join('')}
                </div>
              ` : ''}
            </div>
          `;
        }).join('')}

        <div class="version-divider"><span>or</span></div>

        <div class="custom-llamafile-card ${state.llamafileSource === 'custom' ? 'active' : ''}"
             ${state.llamafileSource === 'custom' ? '' : 'data-action="selectCustomLlamafile"'}>
          <div class="card-icon" style="background: var(--bg-card-active); font-size: 18px;">\ud83d\udcc2</div>
          <div class="card-info">
            <div class="card-title">Load from disk</div>
            ${state.llamafileSource === 'custom' && state.customLlamafilePath ? `
              <div class="custom-path-display" title="${state.customLlamafilePath}">${state.customLlamafilePath}</div>
            ` : `
              <div class="card-desc">Use a custom llamafile build from your filesystem</div>
            `}
          </div>
          ${state.llamafileSource === 'custom' ? `
            <button class="custom-clear-btn" data-action="selectCustomLlamafile" data-stop-propagation="true">Change</button>
            <button class="custom-clear-btn" data-action="clearCustomLlamafile" data-stop-propagation="true">Clear</button>
          ` : `
            <div class="card-meta" style="color: var(--text-secondary);">Browse&hellip;</div>
          `}
        </div>
      </div>
      <div class="footer">
        <div></div>
        <button class="btn btn-primary" data-action="nextStep">Next &rarr;</button>
      </div>
    </div>
  `;
}

async function updateResolvedPath() {
  if (state.llamafileSource === 'custom') {
    state.resolvedLlamafilePath = state.customLlamafilePath;
  } else if (state.selectedRelease) {
    try {
      const cacheDir = await invoke('get_llamafile_cache_dir');
      // Get filename from download URL
      const filename = getFilenameFromUrl(state.selectedRelease.download_url);
      state.resolvedLlamafilePath = `${cacheDir}/${filename}`;
    } catch (err) {
      console.error('Failed to resolve llamafile path:', err);
      state.resolvedLlamafilePath = null;
    }
  } else {
    state.resolvedLlamafilePath = null;
  }
}

async function selectVersionGroup(name) {
  const group = state.versionGroups.find(g => g.name === name);
  if (!group) return;

  // Select first release in group via Rust
  const version = group.releases[0]?.version;
  if (version) {
    const buildState = await invoke('select_llamafile_version', { groupName: name, version });
    syncFromBuildState(buildState);
  }

  // Keep local references for UI
  state.selectedVersionGroup = group;
  state.selectedRelease = group.releases[0];
  // Auto-expand the selected group to show specific version
  state.expandedGroups[name] = true;
  updateResolvedPath();
  renderCurrentStep();
}

async function selectSpecificRelease(groupName, version) {
  const group = state.versionGroups.find(g => g.name === groupName);
  if (!group) return;
  const release = group.releases.find(r => r.version === version);
  if (!release) return;

  // Call Rust to update build state
  const buildState = await invoke('select_llamafile_version', { groupName, version });
  syncFromBuildState(buildState);

  // Keep local references for UI
  state.selectedVersionGroup = group;
  state.selectedRelease = release;
  updateResolvedPath();
  renderCurrentStep();
}

async function selectCustomLlamafile() {
  try {
    const path = await invoke('select_llamafile_path');
    if (path) {
      // Call Rust to update build state
      const buildState = await invoke('use_custom_llamafile', { path });
      syncFromBuildState(buildState);

      // Keep local references for UI
      state.selectedVersionGroup = null;
      state.selectedRelease = null;
      updateResolvedPath();
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to select custom llamafile:', err);
  }
}

async function clearCustomLlamafile() {
  // Select first version group if available
  if (state.versionGroups.length > 0) {
    await selectVersionGroup(state.versionGroups[0].name);
  } else {
    state.llamafileSource = 'registry';
    state.customLlamafilePath = null;
    updateResolvedPath();
    renderCurrentStep();
  }
}

function toggleVersionGroup(name) {
  state.expandedGroups[name] = !state.expandedGroups[name];
  renderCurrentStep();
}

// ============================================================================
// Step 2: Model Selection
// ============================================================================

function getModelFilterChips() {
  const chips = [{ label: 'All', value: 'all' }];
  const tagSet = new Set();
  for (const m of state.models) {
    for (const t of m.tags) {
      tagSet.add(t);
    }
  }
  // Sort tags alphabetically and capitalize
  const sortedTags = [...tagSet].sort();
  for (const t of sortedTags) {
    chips.push({ label: t.charAt(0).toUpperCase() + t.slice(1), value: t });
  }
  // Param-range chips
  chips.push({ label: '\u2264 7B', value: '\u2264 7b' });
  chips.push({ label: '7\u201313B', value: '7\u201313b' });
  chips.push({ label: '\u2265 30B', value: '\u2265 30b' });
  return chips;
}

function initModelFileSelections(model) {
  state.modelFileSelections = {};
  state.expandedModelFiles = {};
  if (!model || !model.files) return;
  for (const f of model.files) {
    state.modelFileSelections[f.role] = {
      source: 'registry',
      quantizationIdx: 0,
      customPath: null,
    };
    state.expandedModelFiles[f.role] = false;
  }
}

function renderModelStep() {
  const chips = getModelFilterChips();
  const isCustomMode = state.modelSource === 'custom';

  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">Choose Model</div>
        <div class="content-subtitle">Select the model weights to bundle from Hugging Face.</div>
      </div>
      <div class="content-body">
        <div class="search-bar">
          <span class="search-icon">\ud83d\udd0d</span>
          <input type="text" class="search-input" placeholder="Search models\u2026"
                 value="${state.searchQuery}" data-action="searchModels">
        </div>

        <div class="filter-row">
          ${chips.map(c => `
            <div class="filter-chip ${state.activeFilter === c.value ? 'active' : ''}"
                 data-action="filterModels" data-filter="${c.value}">${c.label}</div>
          `).join('')}
        </div>

        <div class="download-dir-row">
          <span class="download-dir-path" title="${state.modelDownloadDir}">Download model files in: ${state.modelDownloadDir}</span>
          <button class="download-dir-btn" data-action="selectDownloadDir" data-stop-propagation="true">\ud83d\udcc2</button>
        </div>

        ${state.filteredModels.map(m => {
          const isSelected = !isCustomMode && state.selectedModel?.id === m.id;
          const weightsFile = m.files.find(f => f.role === 'weights');
          const firstQuant = weightsFile?.quantizations[0];
          const totalSize = getTotalModelSizeForModel(m);

          return `
            <div class="model-card-wrapper">
              <div class="selection-card ${isSelected ? 'selected' : ''}"
                   data-action="selectModel" data-model-id="${m.id}">
                <div class="radio-dot"></div>
                <div class="card-info">
                  <div class="card-title">${m.huggingface_url
                    ? `<a href="${m.huggingface_url}" class="model-link" data-action="openExternalLink" data-url="${m.huggingface_url}" data-stop-propagation="true">${m.name}</a>`
                    : m.name
                  }</div>
                  <div class="model-detail">
                    <span>${m.organization}</span>
                    <span>${m.parameters} params</span>
                    ${m.tags.map(t => `<span class="tag-badge">${t}</span>`).join('')}
                    <span class="quant-badge">${firstQuant?.name || 'N/A'}</span>
                  </div>
                </div>
                <div class="card-meta">${formatBytes(totalSize)}</div>
              </div>
              ${isSelected ? renderModelFileConfig(m) : ''}
            </div>
          `;
        }).join('')}

        <div class="version-divider"><span>or</span></div>

        ${renderCustomModelCard()}

        ${state.shaVerifyProgress ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">Calculating SHA256...</div>
              <div class="hf-token-desc">
                Verifying <code>${state.shaVerifyProgress.filename}</code><br>
                <span style="color: var(--text-secondary)">This may take a moment for large files.</span>
              </div>
            </div>
          </div>
        ` : ''}

        ${state.shaVerifyPrompt?.verified ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">SHA256 Verified!</div>
              <div class="hf-token-desc">
                <code>${state.shaVerifyPrompt.filename}</code> matches the expected checksum for <strong>${state.shaVerifyPrompt.quantName}</strong>.<br><br>
                <span style="color: var(--green)">File will be used as ${state.shaVerifyPrompt.quantName} quantization.</span>
              </div>
              <div class="hf-token-buttons" style="margin-top: 16px;">
                <button class="btn btn-primary" data-action="handleShaVerifyChoice" data-choice="ok">OK</button>
              </div>
            </div>
          </div>
        ` : ''}

        ${state.shaVerifyPrompt && !state.shaVerifyPrompt.mismatch && !state.shaVerifyPrompt.verified ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">Verify File Checksum?</div>
              <div class="hf-token-desc">
                <code>${state.shaVerifyPrompt.filename}</code><br><br>
                The registry has a SHA256 checksum for <strong>${state.shaVerifyPrompt.quantName}</strong>.<br>
                Would you like to verify your file matches?
              </div>
              <div class="hf-token-buttons" style="margin-top: 16px;">
                <button class="btn btn-primary" data-action="handleShaVerifyChoice" data-choice="verify">Verify SHA256</button>
                <button class="btn btn-secondary" data-action="handleShaVerifyChoice" data-choice="skip">Skip (use as custom)</button>
                <button class="btn btn-secondary" data-action="handleShaVerifyChoice" data-choice="cancel" style="margin-left: auto;">Cancel</button>
              </div>
            </div>
          </div>
        ` : ''}

        ${state.shaVerifyPrompt?.mismatch ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">SHA256 Mismatch</div>
              <div class="hf-token-desc">
                <code>${state.shaVerifyPrompt.filename}</code> does not match the expected checksum for <strong>${state.shaVerifyPrompt.quantName}</strong>.<br><br>
                <span style="color: var(--text-secondary); font-size: 11px;">
                  Expected: ${state.shaVerifyPrompt.expectedSha?.substring(0, 16)}...<br>
                  Actual: ${state.shaVerifyPrompt.actualSha?.substring(0, 16)}...
                </span><br><br>
                <span style="color: var(--accent)">This file will be used as a custom model instead.</span>
              </div>
              <div class="hf-token-buttons" style="margin-top: 16px;">
                <button class="btn btn-primary" data-action="handleShaVerifyChoice" data-choice="use_custom">OK, Use as Custom</button>
              </div>
            </div>
          </div>
        ` : ''}
      </div>
      <div class="footer">
        <button class="btn btn-secondary" data-action="prevStep">\u2190 Back</button>
        <button class="btn btn-primary" data-action="nextStep" ${!isModelStepValid() ? 'disabled' : ''}>Next \u2192</button>
      </div>
    </div>
  `;
}

function renderModelFileConfig(model) {
  if (!model || !model.files) return '';

  return `
    <div class="model-file-config">
      ${model.files.map(f => {
        const sel = state.modelFileSelections[f.role];
        if (!sel) return '';
        const isExpanded = !!state.expandedModelFiles[f.role];
        const hasMultipleQuants = f.quantizations.length > 1;
        const currentQuant = f.quantizations[sel.quantizationIdx] || f.quantizations[0];
        const isCustomFile = sel.source === 'custom';

        return `
          <div class="file-role-section">
            <div class="file-role-header">
              <span class="file-role-label">${f.label || f.role}</span>
              ${!isCustomFile ? `
                <span class="quant-badge">${currentQuant.name}</span>
                <span class="file-size-label">${formatBytes(currentQuant.size_bytes)}</span>
                ${hasMultipleQuants ? `
                  <div class="expand-toggle" data-action="toggleModelFileExpand" data-role="${f.role}" data-stop-propagation="true">
                    <span class="expand-icon ${isExpanded ? 'expanded' : ''}">\u25b6</span>
                    <span class="expand-count">${f.quantizations.length}</span>
                  </div>
                ` : ''}
                <button class="file-or-disk" data-action="selectCustomFileForRole" data-role="${f.role}" data-stop-propagation="true">or load from disk</button>
              ` : `
                <span class="custom-path-display" title="${sel.customPath || ''}">${getFileName(sel.customPath)}</span>
                <button class="custom-clear-btn" data-action="clearCustomFileForRole" data-role="${f.role}" data-stop-propagation="true">Clear</button>
              `}
            </div>
            ${!isCustomFile && hasMultipleQuants && isExpanded ? `
              <div class="version-releases">
                ${f.quantizations.map((q, qi) => {
                  const isQSelected = sel.quantizationIdx === qi;
                  return `
                    <div class="release-row ${isQSelected ? 'selected' : ''}"
                         data-action="selectFileQuantization" data-role="${f.role}" data-index="${qi}">
                      <div class="release-radio"></div>
                      <div class="release-info">
                        <span class="release-version">${q.name}</span>
                      </div>
                      <span class="release-date">${formatBytes(q.size_bytes)}</span>
                    </div>
                  `;
                }).join('')}
              </div>
            ` : ''}
          </div>
        `;
      }).join('')}
    </div>
  `;
}

function renderCustomModelCard() {
  const isActive = state.modelSource === 'custom';
  const hasFiles = state.customModelFiles.length > 0;

  if (isActive && hasFiles) {
    return `
      <div class="custom-llamafile-card active">
        <div class="card-icon" style="background: var(--bg-card-active); font-size: 18px;">\ud83d\udcc2</div>
        <div class="card-info">
          <div class="card-title">Load from disk</div>
          <div class="custom-file-list">
            ${state.customModelFiles.map((f, i) => `
              <div class="custom-file-entry">
                <span class="file-role-tag">${f.role}</span>
                <span class="custom-path-display" title="${f.path}">${getFileName(f.path)}</span>
              </div>
            `).join('')}
          </div>
          <button class="custom-add-btn" data-action="addCompanionFile" data-stop-propagation="true">+ Add file</button>
        </div>
        <button class="custom-clear-btn" data-action="clearCustomModel" data-stop-propagation="true">Clear</button>
      </div>
    `;
  }

  return `
    <div class="custom-llamafile-card ${isActive ? 'active' : ''}"
         data-action="selectCustomModel">
      <div class="card-icon" style="background: var(--bg-card-active); font-size: 18px;">\ud83d\udcc2</div>
      <div class="card-info">
        <div class="card-title">Load from disk</div>
        <div class="card-desc">Use a custom GGUF model file from your filesystem</div>
      </div>
      <div class="card-meta" style="color: var(--text-secondary);">Browse&hellip;</div>
    </div>
  `;
}

// -- Model interaction functions --

async function selectModel(modelId) {
  const model = state.models.find(m => m.id === modelId);
  if (!model) return;

  // Call Rust to update build state
  const buildState = await invoke('select_registry_model', { modelId });
  syncFromBuildState(buildState);

  // Keep model object reference for UI (Rust only stores the ID)
  state.selectedModel = model;

  // Initialize UI-only state
  initModelFileSelections(model);
  renderCurrentStep();
}

function toggleModelFileExpand(role) {
  state.expandedModelFiles[role] = !state.expandedModelFiles[role];
  renderCurrentStep();
}

async function selectFileQuantization(role, idx) {
  // Call Rust to update build state
  const buildState = await invoke('select_file_quantization', { role, idx });
  syncFromBuildState(buildState);

  // Keep local UI state in sync
  if (state.modelFileSelections[role]) {
    state.modelFileSelections[role].quantizationIdx = idx;
    state.modelFileSelections[role].source = 'registry';
    state.modelFileSelections[role].customPath = null;
  }
  renderCurrentStep();
}

// Output path is now managed by Rust BuildState
// This function is kept for backwards compatibility but just syncs from Rust
async function updateDefaultOutputPath() {
  const buildState = await invoke('get_build_state');
  state.outputPath = buildState.outputPath;
  renderCurrentStep();
}

async function selectCustomFileForRole(role) {
  try {
    const path = await invoke('select_model_file_path');
    if (!path || !state.modelFileSelections[role]) return;

    const filename = path.split('/').pop();
    const sel = state.modelFileSelections[role];
    const file = state.selectedModel?.files.find(f => f.role === role);
    const quant = file?.quantizations[sel.quantizationIdx];
    const expectedSha = quant?.sha256;

    // If no expected SHA in registry, just use as custom file
    if (!expectedSha) {
      // Call Rust to update build state
      const buildState = await invoke('use_custom_file_for_role', { role, path });
      syncFromBuildState(buildState);

      // Update local UI state
      sel.source = 'custom';
      sel.customPath = path;
      renderCurrentStep();
      return;
    }

    // Ask user if they want to verify SHA
    const action = await new Promise((resolve) => {
      state.shaVerifyPrompt = { role, path, filename, expectedSha, quantName: quant.name, resolve };
      renderCurrentStep();
    });

    if (action === 'skip') {
      // User doesn't want to verify - switch to full custom model mode
      switchToCustomModelWithFile(role, path);
    } else if (action === 'verify') {
      // Calculate SHA and verify
      state.shaVerifyPrompt = null;
      state.shaVerifyProgress = { filename };
      renderCurrentStep();

      try {
        const actualSha = await invoke('calculate_sha256', { path });
        state.shaVerifyProgress = null;

        if (actualSha === expectedSha) {
          // SHA matches - keep as registry model with custom file
          // Call Rust to update build state
          const buildState = await invoke('use_custom_file_for_role', { role, path });
          syncFromBuildState(buildState);

          // Update local UI state
          sel.source = 'custom';
          sel.customPath = path;

          // Show success modal
          state.shaVerifyPrompt = {
            role, path, filename, expectedSha, quantName: quant.name,
            verified: true,
            resolve: () => {
              state.shaVerifyPrompt = null;
              renderCurrentStep();
            }
          };
          renderCurrentStep();
        } else {
          // SHA doesn't match - switch to custom model mode
          state.shaVerifyPrompt = {
            role, path, filename, expectedSha, quantName: quant.name,
            mismatch: true, actualSha,
            resolve: (action) => {
              state.shaVerifyPrompt = null;
              if (action === 'use_custom') {
                switchToCustomModelWithFile(role, path);
              }
              renderCurrentStep();
            }
          };
          renderCurrentStep();
        }
      } catch (err) {
        state.shaVerifyProgress = null;
        console.error('Failed to calculate SHA:', err);
        // On error, just use as custom
        switchToCustomModelWithFile(role, path);
      }
    } else {
      // Cancelled
      state.shaVerifyPrompt = null;
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to select custom file for role:', err);
  }
}

async function switchToCustomModelWithFile(role, path) {
  // Call Rust to switch to custom model mode
  const buildState = await invoke('select_custom_model', { path });
  syncFromBuildState(buildState);
  renderCurrentStep();
}

function handleShaVerifyChoice(choice) {
  if (state.shaVerifyPrompt?.resolve) {
    state.shaVerifyPrompt.resolve(choice);
  }
}

async function clearCustomFileForRole(role) {
  // Call Rust to clear custom file
  const buildState = await invoke('clear_custom_file_for_role', { role });
  syncFromBuildState(buildState);

  // Update local UI state
  if (state.modelFileSelections[role]) {
    state.modelFileSelections[role].source = 'registry';
    state.modelFileSelections[role].customPath = null;
    state.modelFileSelections[role].quantizationIdx = 0;
  }
  renderCurrentStep();
}

async function selectCustomModel() {
  try {
    const path = await invoke('select_model_file_path');
    if (path) {
      // Call Rust to switch to custom model mode
      const buildState = await invoke('select_custom_model', { path });
      syncFromBuildState(buildState);

      // Clear UI-only state
      state.modelFileSelections = {};
      state.expandedModelFiles = {};
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to select custom model:', err);
  }
}

async function addCompanionFile() {
  try {
    const path = await invoke('select_model_file_path');
    if (path) {
      // Assign role based on count: second file is mmproj, subsequent are auxiliary
      const existingRoles = state.customModelFiles.map(f => f.role);
      let role = 'auxiliary';
      if (!existingRoles.includes('mmproj')) {
        role = 'mmproj';
      }

      // Call Rust to add companion file
      const buildState = await invoke('add_custom_companion_file', { role, path });
      syncFromBuildState(buildState);
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to add companion file:', err);
  }
}

async function removeCompanionFile(role) {
  const buildState = await invoke('remove_custom_companion_file', { role });
  syncFromBuildState(buildState);
  renderCurrentStep();
}

async function clearCustomModel() {
  // Re-select first model if available (this will switch back to registry mode)
  if (state.models.length > 0) {
    await selectModel(state.models[0].id);
  } else {
    state.modelSource = 'registry';
    state.selectedModel = null;
    state.customModelFiles = [];
    renderCurrentStep();
  }
}

async function selectDownloadDir() {
  try {
    const path = await invoke('select_model_download_dir');
    if (path) {
      state.modelDownloadDir = path;
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to select download directory:', err);
  }
}

function isModelStepValid() {
  if (state.modelSource === 'custom') {
    return state.customModelFiles.length > 0 && state.customModelFiles[0].path;
  }
  return !!state.selectedModel;
}

function getTotalModelSize() {
  if (state.modelSource === 'custom') {
    return 0; // Unknown for custom files
  }
  if (!state.selectedModel) return 0;
  return getTotalModelSizeForModel(state.selectedModel);
}

function getTotalModelSizeForModel(model) {
  let total = 0;
  for (const f of model.files) {
    const sel = state.modelFileSelections[f.role];
    if (sel && sel.source === 'registry') {
      const q = f.quantizations[sel.quantizationIdx] || f.quantizations[0];
      total += q.size_bytes;
    }
    // Custom files: size unknown, skip
    if (!sel) {
      // No selection state yet, use first quant
      total += f.quantizations[0]?.size_bytes || 0;
    }
  }
  return total;
}

function renderModelSummaryValue() {
  if (state.modelSource === 'custom') {
    const files = state.customModelFiles;
    if (files.length === 0) return 'Custom \u2014 N/A';
    const primary = getFileName(files[0].path);
    const companions = files.slice(1).map(f => `${f.role}: ${getFileName(f.path)}`);
    let text = `Custom \u2014 ${primary}`;
    if (companions.length > 0) {
      text += ` + ${companions.join(', ')}`;
    }
    return text;
  }
  if (!state.selectedModel) return 'N/A';
  const weightsFile = state.selectedModel.files.find(f => f.role === 'weights');
  const weightsSel = state.modelFileSelections['weights'];
  const quantName = weightsSel
    ? (weightsFile?.quantizations[weightsSel.quantizationIdx]?.name || 'N/A')
    : (weightsFile?.quantizations[0]?.name || 'N/A');
  let text = `${state.selectedModel.name} (${quantName})`;
  // Show companion files
  const companions = state.selectedModel.files.filter(f => f.role !== 'weights');
  if (companions.length > 0) {
    const companionParts = companions.map(f => {
      const sel = state.modelFileSelections[f.role];
      if (sel?.source === 'custom') return `${f.label || f.role}: custom`;
      const q = sel ? f.quantizations[sel.quantizationIdx] : f.quantizations[0];
      return `${f.label || f.role}: ${q?.name || 'N/A'}`;
    });
    text += ` + ${companionParts.join(', ')}`;
  }
  return text;
}

function searchModels(query) {
  state.searchQuery = query;
  applyModelFilters();
}

function filterModels(filter) {
  state.activeFilter = filter;
  applyModelFilters();
}

function applyModelFilters() {
  let filtered = [...state.models];

  if (state.searchQuery) {
    const q = state.searchQuery.toLowerCase();
    filtered = filtered.filter(m =>
      m.name.toLowerCase().includes(q) ||
      m.organization.toLowerCase().includes(q)
    );
  }

  if (state.activeFilter !== 'all') {
    const f = state.activeFilter;
    switch (f) {
      case '\u2264 7b':
        filtered = filtered.filter(m => m.param_count <= 7e9);
        break;
      case '7\u201313b':
        filtered = filtered.filter(m => m.param_count > 7e9 && m.param_count <= 13e9);
        break;
      case '\u2265 30b':
        filtered = filtered.filter(m => m.param_count >= 30e9);
        break;
      default:
        // Tag-based filter
        filtered = filtered.filter(m => m.tags.some(t => t === f));
        break;
    }
  }

  state.filteredModels = filtered;
  renderCurrentStep();
}

// ============================================================================
// Step 3: Configuration
// ============================================================================

function renderConfigStep() {
  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">Configuration</div>
        <div class="content-subtitle">Set runtime parameters for the <code>.args</code> file.</div>
      </div>
      <div class="content-body">
        ${renderConfigItems()}
      </div>
      <div class="footer">
        <button class="btn btn-secondary" data-action="prevStep">\u2190 Back</button>
        <button class="btn btn-primary" data-action="nextStep">Next \u2192</button>
      </div>
    </div>
  `;
}

function renderConfigItems() {
  return renderConfigItemsRecursive(state.configSchema);
}

function renderConfigItemsRecursive(items) {
  let html = '';

  for (const item of items) {
    // Skip hidden items - they're only used at build time
    if (item.hidden) continue;

    if (item.type === 'section') {
      // Check if this is a toggleable section (has a key)
      if (item.key) {
        const enabled = state.configValues[item.key] ?? item.default ?? true;
        html += `
          <div class="config-section config-section-toggleable">
            <div class="config-section-header" data-action="toggleConfig" data-config-key="${item.key}">
              <div class="config-section-info">
                <div class="config-section-title">${item.label}</div>
                ${item.description ? `<div class="config-section-desc">${item.description}</div>` : ''}
              </div>
              <div class="toggle-switch ${enabled ? 'on' : ''}"></div>
            </div>
            ${enabled && item.items ? `<div class="config-section-items">${renderConfigItemsRecursive(item.items)}</div>` : ''}
          </div>
        `;
      } else {
        // Regular section header (not toggleable)
        html += `
          <div class="config-section">
            <div class="config-section-title">${item.label}</div>
            ${item.items ? renderConfigItemsRecursive(item.items) : ''}
          </div>
        `;
      }
    } else if (item.type === 'text') {
      html += renderTextItem(item);
    } else if (item.type === 'dropdown') {
      html += renderDropdownItem(item);
    } else if (item.type === 'bool') {
      html += renderBoolItem(item);
    } else if (item.type === 'slider') {
      html += renderSliderItem(item);
    }
  }

  return html;
}

function renderTextItem(item) {
  const val = state.configValues[item.key];
  const inputType = item.value_type === 'number' ? 'number' : 'text';
  const valueType = item.value_type === 'number' ? 'number' : 'text';
  return `
    <div class="config-row">
      <div class="config-label">${item.label}<div class="config-label-sub">${item.description}</div></div>
      <input class="config-input" type="${inputType}" value="${val}" data-action="updateConfig" data-config-key="${item.key}" data-value-type="${valueType}">
    </div>
  `;
}

function renderDropdownItem(item) {
  const val = state.configValues[item.key];
  const isNumeric = item.options.length > 0 && typeof item.options[0] === 'number';
  const valueType = isNumeric ? 'number' : 'text';
  return `
    <div class="config-row">
      <div class="config-label">${item.label}<div class="config-label-sub">${item.description}</div></div>
      <select class="config-select" data-action="updateConfig" data-config-key="${item.key}" data-value-type="${valueType}">
        ${item.options.map(o => {
          const display = typeof o === 'number' ? o.toLocaleString() : o;
          return `<option value="${o}" ${val === o ? 'selected' : ''}>${display}</option>`;
        }).join('')}
      </select>
    </div>
  `;
}

function renderBoolItem(item) {
  const val = state.configValues[item.key];
  return `
    <div class="toggle-row" data-action="toggleConfig" data-config-key="${item.key}">
      <div class="toggle-info">
        <div class="toggle-title">${item.label}</div>
        <div class="toggle-desc">${item.description}</div>
      </div>
      <div class="toggle-switch ${val ? 'on' : ''}"></div>
    </div>
  `;
}

function renderSliderItem(item) {
  const val = state.configValues[item.key];
  const step = item.step ?? 1;
  const isInteger = step >= 1 && Number.isInteger(step);
  return `
    <div class="config-row">
      <div class="config-label">${item.label}<div class="config-label-sub">${item.description}</div></div>
      <div class="slider-container">
        <input type="range" class="config-slider"
          min="${item.min}" max="${item.max}" step="${step}" value="${val}"
          data-action="updateSlider" data-config-key="${item.key}" data-is-integer="${isInteger}">
        <span class="slider-value" id="slider-value-${item.key}">${isInteger ? Math.round(val) : val}</span>
      </div>
    </div>
  `;
}

async function updateConfig(key, value) {
  state.configValues[key] = value;
  // Update Rust build state (fire-and-forget for performance)
  invoke('set_config_value', { key, value: value });
}

function updateSlider(key, value, isInteger) {
  const numValue = isInteger ? parseInt(value) : parseFloat(value);
  state.configValues[key] = numValue;
  // Update the displayed value
  const valueEl = document.getElementById(`slider-value-${key}`);
  if (valueEl) {
    valueEl.textContent = isInteger ? Math.round(numValue) : numValue;
  }
  // Update Rust build state
  invoke('set_config_value', { key, value: numValue });
}

async function toggleConfig(key) {
  const newValue = !state.configValues[key];
  state.configValues[key] = newValue;
  // Update Rust build state
  invoke('set_config_value', { key, value: newValue });
  renderCurrentStep();
}

// ============================================================================
// Step 4: Compute
// ============================================================================

function renderComputeStep() {
  const estimatedSize = estimateTotalSize();
  const sizePercent = Math.min((estimatedSize / 10e9) * 100, 100);

  // Check if GPU libs are available for the selected release (any platform)
  const availableGpuLibs = getGpuLibsForRelease(state.selectedRelease);
  const hasAnyCudaOrRocm = availableGpuLibs.cuda || availableGpuLibs.rocm;

  // Get actual sizes and platform info from release metadata
  const cudaSize = availableGpuLibs.cuda?.totalSize;
  const cudaPlatforms = availableGpuLibs.cuda?.platforms.join(', ') || '';
  const rocmSize = availableGpuLibs.rocm?.totalSize;
  const rocmPlatforms = availableGpuLibs.rocm?.platforms.join(', ') || '';

  // GPU is always available: Metal (macOS) is built-in, CUDA/ROCm depend on release
  const gpuAvailable = true;

  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">Compute Target</div>
        <div class="content-subtitle">Choose whether to bundle GPU acceleration libraries for your target platforms.</div>
      </div>
      <div class="content-body">
        <div class="compute-grid">
          <div class="compute-card ${state.computeMode === 'cpu' ? 'selected' : ''}" data-action="selectComputeMode" data-mode="cpu">
            <div class="compute-card-icon">\ud83e\uddf3</div>
            <div class="compute-card-title">CPU Only</div>
            <div class="compute-card-desc">Lean build, runs on any x86_64 or ARM64 machine. Passes --gpu DISABLE to prevent Metal compilation.</div>
            <div class="compute-card-meta">
              <span class="meta-chip">x86_64</span>
              <span class="meta-chip">ARM64</span>
            </div>
          </div>
          <div class="compute-card ${state.computeMode === 'gpu' ? 'selected' : ''}" data-action="selectComputeMode" data-mode="gpu">
            <div class="compute-card-icon">\ud83d\udd25</div>
            <div class="compute-card-title">GPU Accelerated</div>
            <div class="compute-card-desc">Bundle GPU libraries for target platforms.</div>
            <div class="compute-card-meta">
              <span class="meta-chip">Metal</span>
              ${availableGpuLibs.cuda ? '<span class="meta-chip">CUDA</span>' : ''}
              ${availableGpuLibs.rocm ? '<span class="meta-chip">ROCm</span>' : ''}
            </div>
          </div>
        </div>

        <div class="config-section" style="opacity: ${state.computeMode === 'gpu' ? 1 : 0.3}; pointer-events: ${state.computeMode === 'gpu' ? 'all' : 'none'};">
          <div class="config-section-title">GPU Options</div>
          <div class="toggle-row" data-action="toggleGpuOption" data-option="metal">
            <div class="toggle-info"><div class="toggle-title">Apple Metal</div><div class="toggle-desc">macOS — Built-in (compiles on first run)</div></div>
            <div class="toggle-switch ${state.gpuOptions.metal ? 'on' : ''}"></div>
          </div>
          ${availableGpuLibs.cuda ? `
            <div class="toggle-row" data-action="toggleGpuOption" data-option="cuda">
              <div class="toggle-info"><div class="toggle-title">NVIDIA CUDA</div><div class="toggle-desc">${cudaPlatforms} — ${formatBytes(cudaSize)}</div></div>
              <div class="toggle-switch ${state.gpuOptions.cuda ? 'on' : ''}"></div>
            </div>
          ` : `
            <div class="toggle-row" style="opacity: 0.5; cursor: not-allowed;">
              <div class="toggle-info"><div class="toggle-title">NVIDIA CUDA</div><div class="toggle-desc">Not available for this version</div></div>
              <div class="toggle-switch"></div>
            </div>
          `}
          ${availableGpuLibs.rocm ? `
            <div class="toggle-row" data-action="toggleGpuOption" data-option="rocm">
              <div class="toggle-info"><div class="toggle-title">AMD ROCm</div><div class="toggle-desc">${rocmPlatforms} — ${formatBytes(rocmSize)}</div></div>
              <div class="toggle-switch ${state.gpuOptions.rocm ? 'on' : ''}"></div>
            </div>
          ` : `
            <div class="toggle-row" style="opacity: 0.5; cursor: not-allowed;">
              <div class="toggle-info"><div class="toggle-title">AMD ROCm</div><div class="toggle-desc">Not available for this version</div></div>
              <div class="toggle-switch"></div>
            </div>
          `}
        </div>

        <div class="size-estimate">
          <div class="size-label">Estimated total size</div>
          <div class="size-bar"><div class="size-fill" style="width: ${sizePercent}%"></div></div>
          <div class="size-text">${formatBytes(estimatedSize)}</div>
        </div>
      </div>
      <div class="footer">
        <button class="btn btn-secondary" data-action="prevStep">\u2190 Back</button>
        <button class="btn btn-primary" data-action="nextStep">Next \u2192</button>
      </div>
    </div>
  `;
}

function selectComputeMode(mode) {
  state.computeMode = mode;
  renderCurrentStep();
}

function toggleGpuOption(opt) {
  // Only toggle if the option is available
  const availableGpuLibs = getGpuLibsForRelease(state.selectedRelease);
  if (opt === 'cuda' && !availableGpuLibs.cuda) return;
  if (opt === 'rocm' && !availableGpuLibs.rocm) return;
  state.gpuOptions[opt] = !state.gpuOptions[opt];
  renderCurrentStep();
}

function estimateTotalSize() {
  const modelSize = getTotalModelSize();
  // Use actual runtime size from selected release
  let overhead = state.selectedRelease?.size_bytes || 0;

  if (state.computeMode === 'gpu') {
    const availableGpuLibs = getGpuLibsForRelease(state.selectedRelease);
    // Use actual sizes from release metadata when available
    if (state.gpuOptions.cuda && availableGpuLibs.cuda) {
      overhead += availableGpuLibs.cuda.totalSize;
    }
    if (state.gpuOptions.rocm && availableGpuLibs.rocm) {
      overhead += availableGpuLibs.rocm.totalSize;
    }
    if (state.gpuOptions.metal) {
      overhead += 10e6; // Metal sources are small
    }
  }
  if (state.agentEnabled) overhead += 1e6;
  return modelSize + overhead;
}

// ============================================================================
// Step 5: Agent
// ============================================================================

function renderAgentStep() {
  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">Agent Configuration</div>
        <div class="content-subtitle">Optionally build an agentfile with tools and custom instructions.</div>
      </div>
      <div class="content-body">
        <div class="agent-toggle-big ${state.agentEnabled ? 'active' : ''}" data-action="toggleAgent">
          <div>
            <div style="font-size: 13px; font-weight: 600; margin-bottom: 2px;">Enable Agentfile Mode</div>
            <div style="font-size: 11px; color: var(--text-secondary);">Bundle tool definitions and agent instructions</div>
          </div>
          <div class="toggle-switch ${state.agentEnabled ? 'on' : ''}"></div>
        </div>

        <div class="agent-section ${state.agentEnabled ? 'enabled' : ''}">
          <div class="config-section-title">Available Tools</div>
          <div class="tool-grid">
            ${state.agentTools.map(t => `
              <div class="tool-card ${t.enabled ? 'selected' : ''}" data-action="toggleTool" data-tool-id="${t.id}">
                <div class="tool-icon">${t.icon}</div>
                <div class="tool-name">${t.name}</div>
                <div class="tool-check">${t.enabled ? '\u2713' : ''}</div>
              </div>
            `).join('')}
          </div>

          <div class="config-section-title">System Instructions</div>
          <div class="code-editor">
            <div class="code-editor-header">
              <div class="code-editor-tab">\ud83d\udcc4 system_prompt.md</div>
            </div>
            <div class="code-editor-body">
              <textarea data-action="updateSystemPrompt">${state.systemPrompt}</textarea>
            </div>
          </div>
        </div>
      </div>
      <div class="footer">
        <button class="btn btn-secondary" data-action="prevStep">\u2190 Back</button>
        <button class="btn btn-primary" data-action="nextStep">Next \u2192</button>
      </div>
    </div>
  `;
}

function toggleAgent() { state.agentEnabled = !state.agentEnabled; renderCurrentStep(); }
function toggleTool(id) { const t = state.agentTools.find(x => x.id === id); if (t) { t.enabled = !t.enabled; renderCurrentStep(); } }
function updateSystemPrompt(v) { state.systemPrompt = v; }

// ============================================================================
// Step 6: Build
// ============================================================================

// Get the filename for a model file by role (used to pass to Rust)
function getModelFilename(role) {
  if (state.modelSource === 'custom') {
    const file = state.customModelFiles.find(f => f.role === role);
    return file ? getFileName(file.path) : null;
  }
  if (!state.selectedModel) return null;
  const file = state.selectedModel.files.find(f => f.role === role);
  if (!file) return null;
  const sel = state.modelFileSelections[role];
  if (sel?.source === 'custom' && sel.customPath) {
    return getFileName(sel.customPath);
  }
  const quant = file.quantizations[sel?.quantizationIdx || 0];
  // Extract filename from download URL
  return quant ? getFilenameFromUrl(quant.download_url) : null;
}

// Regenerate args content by calling Rust backend
async function refreshArgsContent() {
  try {
    const modelFilename = getModelFilename('weights') || 'model.gguf';
    const mmprojFilename = getModelFilename('mmproj') || null;

    state.argsContent = await invoke('generate_args_content', {
      configValues: state.configValues,
      modelFilename,
      mmprojFilename,
    });
  } catch (err) {
    console.error('Failed to generate args content:', err);
    state.argsContent = '# Error generating .args content\n...';
  }
}

// Called when user edits the args textarea
function updateArgsContent(value) {
  state.argsContent = value;
}

function renderBuildStep() {
  const enabledTools = state.agentTools.filter(t => t.enabled);

  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">Build</div>
        <div class="content-subtitle">Review configuration and build the llamafile.</div>
      </div>
      <div class="content-body">
        <div class="build-summary">
          <div class="summary-row">
            <div class="summary-label">Runtime</div>
            <div class="summary-value">${
              state.llamafileSource === 'custom'
                ? `Custom \u2014 <span class="custom-path-display" style="display:inline" title="${state.customLlamafilePath || ''}">${state.customLlamafilePath || 'N/A'}</span>`
                : `llamafile ${state.selectedRelease?.version || 'N/A'}`
            }</div>
            <div class="summary-edit" data-action="goToStep" data-step="1">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Model</div>
            <div class="summary-value">${renderModelSummaryValue()}</div>
            <div class="summary-edit" data-action="goToStep" data-step="2">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Context</div>
            <div class="summary-value">${(state.configValues.context_length || 4096).toLocaleString()} tokens</div>
            <div class="summary-edit" data-action="goToStep" data-step="3">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Compute</div>
            <div class="summary-value">${state.computeMode === 'gpu' ? 'GPU (' + getGpuSummary() + ')' : 'CPU Only'}</div>
            <div class="summary-edit" data-action="goToStep" data-step="4">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Agent</div>
            <div class="summary-value">${state.agentEnabled ? `Enabled \u2014 ${enabledTools.length} tools` : 'Disabled'}</div>
            <div class="summary-edit" data-action="goToStep" data-step="5">edit</div>
          </div>
        </div>

        <div class="output-path" data-action="selectOutputPath">
          <div class="output-path-label">Output:</div>
          <div class="output-path-value">${state.outputPath || 'Select location...'}</div>
          <div class="output-path-btn">Browse\u2026</div>
        </div>

        <div class="args-preview">
          <div class="args-preview-header">
            <div class="args-preview-label">.args Configuration</div>
            <button class="args-preview-refresh" data-action="refreshArgsContent" title="Regenerate from settings">Refresh</button>
          </div>
          <textarea
            class="args-preview-editor"
            spellcheck="false"
            data-action="updateArgsContent"
          >${escapeHtml(state.argsContent)}</textarea>
        </div>

        ${state.isBuilding || state.buildProgress ? renderBuildProgress() : ''}

        ${state.downloadProgress ? `
          <div class="download-progress">
            <div class="download-progress-label">Downloading:</div>
            <div class="download-progress-text" id="download-progress-text">${state.downloadProgress.filename}: ${formatBytes(state.downloadProgress.bytes_downloaded)} / ${formatBytes(state.downloadProgress.bytes_total)} (${state.downloadProgress.percentage.toFixed(1)}%)</div>
            <div class="download-progress-bar">
              <div class="download-progress-fill" style="width: ${state.downloadProgress.percentage}%"></div>
            </div>
          </div>
        ` : ''}

        ${state.buildLog.length > 0 || state.isBuilding ? `
          <div class="build-log" id="build-log">
            <div class="build-log-header">Log</div>
            <pre class="build-log-content" id="build-log-content">${state.buildLog.length > 0 ? escapeHtml(state.buildLog.join('\n')) : ''}</pre>
          </div>
        ` : ''}

        ${state.hfTokenPromptVisible ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">Authentication Required</div>
              <div class="hf-token-desc">This model requires a Hugging Face token. You can get one at <a href="https://huggingface.co/settings/tokens" target="_blank">huggingface.co/settings/tokens</a></div>
              <input type="password" class="hf-token-input" id="hf-token-input" placeholder="hf_..." data-action="submitHfToken">
              <div class="hf-token-buttons">
                <button class="btn btn-secondary" data-action="cancelHfTokenPrompt">Cancel</button>
                <button class="btn btn-primary" data-action="submitHfToken">Save &amp; Retry</button>
              </div>
            </div>
          </div>
        ` : ''}

        ${state.fileExistsPrompt ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              ${state.fileExistsPrompt.isPartial ? `
                <div class="hf-token-title">Incomplete Download Found</div>
                <div class="hf-token-desc">
                  <code>${state.fileExistsPrompt.filename}</code> appears to be a partial download.<br>
                  <span style="color: var(--text-muted)">
                    ${formatBytes(state.fileExistsPrompt.currentSize)} of ${formatBytes(state.fileExistsPrompt.expectedSize)} downloaded
                  </span>
                </div>
                <div class="hf-token-buttons" style="margin-top: 16px;">
                  <button class="btn btn-primary" data-action="handleFileExistsChoice" data-choice="resume">Resume Download</button>
                  <button class="btn btn-secondary" data-action="handleFileExistsChoice" data-choice="download">Start Fresh</button>
                  <button class="btn btn-secondary" data-action="handleFileExistsChoice" data-choice="cancel" style="margin-left: auto;">Cancel</button>
                </div>
              ` : state.fileExistsPrompt.checksumMismatch ? `
                <div class="hf-token-title">Checksum Mismatch</div>
                <div class="hf-token-desc">
                  <code>${state.fileExistsPrompt.filename}</code> exists but the checksum doesn't match.<br>
                  <span style="color: var(--accent)">The file may be corrupted or from a different version.</span>
                </div>
                <div class="hf-token-buttons" style="margin-top: 16px;">
                  <button class="btn btn-primary" data-action="handleFileExistsChoice" data-choice="download">Re-download</button>
                  <button class="btn btn-secondary" data-action="handleFileExistsChoice" data-choice="skip">Use Anyway</button>
                  <button class="btn btn-secondary" data-action="handleFileExistsChoice" data-choice="cancel" style="margin-left: auto;">Cancel</button>
                </div>
              ` : `
                <div class="hf-token-title">File Already Exists</div>
                <div class="hf-token-desc">
                  <code>${state.fileExistsPrompt.filename}</code> already exists in the cache.${
                    state.fileExistsPrompt.isComplete
                      ? (state.fileExistsPrompt.checksumVerified
                          ? '<br><span style="color: var(--green)">File is complete and checksum verified.</span>'
                          : '<br><span style="color: var(--text-secondary)">File size matches (no checksum available to verify).</span>')
                      : ''
                  }
                </div>
                <div class="hf-token-buttons" style="margin-top: 16px;">
                  <button class="btn btn-primary" data-action="handleFileExistsChoice" data-choice="skip">Use Existing</button>
                  <button class="btn btn-secondary" data-action="handleFileExistsChoice" data-choice="download">Re-download</button>
                </div>
              `}
            </div>
          </div>
        ` : ''}

        ${state.cancelConfirmVisible ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">Cancel Build?</div>
              <div class="hf-token-desc">
                Are you sure you want to cancel the build?${state.downloadProgress ? '<br><br><span style="color: var(--accent)">The current download will be stopped and the partial file will be deleted.</span>' : ''}
              </div>
              <div class="hf-token-buttons" style="margin-top: 16px;">
                <button class="btn btn-secondary" data-action="dismissCancelConfirm">Continue Build</button>
                <button class="btn btn-primary" style="background: var(--red);" data-action="confirmCancelBuild">Cancel Build</button>
              </div>
            </div>
          </div>
        ` : ''}
      </div>
      <div class="footer">
        <button class="btn btn-secondary" data-action="prevStep" ${state.isBuilding ? 'disabled' : ''}>\u2190 Back</button>
        ${renderBuildButton()}
      </div>
    </div>
  `;
}

function getGpuSummary() {
  const parts = [];
  if (state.gpuOptions.cuda) parts.push('CUDA');
  if (state.gpuOptions.rocm) parts.push('ROCm');
  if (state.gpuOptions.metal) parts.push('Metal');
  return parts.join(' + ') || 'None';
}

function renderBuildButton() {
  if (state.buildProgress?.stage === 'complete') {
    return `
      <button class="btn btn-success" data-action="openOutputFolder"><img src="llamafile-icon.png" style="width:16px;height:16px;"> Show in Finder</button>
      <button class="btn btn-primary" data-action="resetBuild" style="margin-left: 8px;">Build Another</button>
    `;
  }
  if (state.buildProgress?.stage === 'error') {
    return `
      <button class="btn btn-primary" data-action="resetBuild">Try Again</button>
    `;
  }
  if (state.isBuilding) {
    return `<button class="btn btn-secondary" data-action="cancelBuild">Cancel</button>`;
  }
  return `<button class="btn btn-primary" data-action="startBuild">\u26a1 Build llamafile</button>`;
}

function resetBuild() {
  state.buildProgress = null;
  state.buildLog = [];
  state.downloadProgress = null;
  state.currentDownloadPath = null;
  renderCurrentStep();
}

function renderBuildProgress() {
  if (!state.buildProgress) return '';

  const stages = [
    { key: 'downloading_model', label: 'Downloading model weights\u2026' },
    { key: 'downloading_runtime', label: 'Fetching llamafile runtime\u2026' },
    { key: 'writing_config', label: 'Writing .args configuration\u2026' },
    { key: 'bundling_gpu', label: 'Bundling GPU libraries\u2026' },
    { key: 'creating_executable', label: 'Creating executable\u2026' },
  ];

  const currentStage = state.buildProgress.stage;
  const currentIdx = stages.findIndex(s => s.key === currentStage);

  return `
    <div class="build-progress">
      <div class="progress-header">
        <div class="progress-label">${currentStage === 'complete' ? 'Build complete!' : 'Building\u2026'}</div>
        <div class="progress-pct" style="color: ${currentStage === 'complete' ? 'var(--green)' : 'var(--accent)'}">${
          currentStage === 'complete' ? '\u2713 Done' : Math.round(state.buildProgress.progress_pct) + '%'
        }</div>
      </div>
      <div class="progress-bar">
        <div class="progress-fill" style="width: ${state.buildProgress.progress_pct}%"></div>
      </div>
      <div class="progress-steps">
        ${stages.map((s, i) => {
          let cls = 'progress-step';
          if (i < currentIdx || currentStage === 'complete') cls += ' done';
          else if (i === currentIdx) cls += ' active';
          return `<div class="${cls}"><div class="progress-step-dot"></div>${s.label}</div>`;
        }).join('')}
      </div>
    </div>
  `;
}

function renderBuildLog() {
  const el = document.getElementById('build-log-content');
  if (!el) return;
  el.textContent = state.buildLog.join('\n');
  el.scrollTop = el.scrollHeight;
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

function renderDownloadProgress() {
  if (!state.downloadProgress) return;

  const { filename, bytes_downloaded, bytes_total, percentage } = state.downloadProgress;
  const downloaded = formatBytes(bytes_downloaded);
  const total = bytes_total > 0 ? formatBytes(bytes_total) : '?';
  const pct = percentage.toFixed(1);

  const textEl = document.getElementById('download-progress-text');
  if (textEl) {
    textEl.textContent = `${filename}: ${downloaded} / ${total} (${pct}%)`;
  }

  const fillEl = document.querySelector('.download-progress-fill');
  if (fillEl) {
    fillEl.style.width = `${percentage}%`;
  }
}

// Download a file, handling auth errors by prompting for HF token
async function downloadWithAuth(url, destPath, expectedSize, sha256, resume = false) {
  try {
    return await invoke('download_file', {
      url,
      destPath,
      expectedSize: expectedSize || null,
      expectedSha256: sha256 || null,
      resume,
    });
  } catch (err) {
    if (err === 'AUTH_REQUIRED') {
      // Store pending retry info and show token prompt
      state.hfTokenPendingRetry = { url, destPath, expectedSize, sha256, resume };
      state.hfTokenPromptVisible = true;
      renderCurrentStep();
      // Wait for user to enter token or cancel
      return new Promise((resolve, reject) => {
        window._hfTokenResolve = resolve;
        window._hfTokenReject = reject;
      });
    }
    throw err;
  }
}

async function submitHfToken(token) {
  if (!token || !token.trim()) {
    cancelHfTokenPrompt();
    return;
  }

  try {
    await invoke('set_hf_token', { token: token.trim() });
    state.hfTokenPromptVisible = false;

    // Retry the pending download
    if (state.hfTokenPendingRetry) {
      const { url, destPath, expectedSize, sha256, resume } = state.hfTokenPendingRetry;
      state.hfTokenPendingRetry = null;
      renderCurrentStep();

      try {
        const result = await invoke('download_file', {
          url,
          destPath,
          expectedSize: expectedSize || null,
          expectedSha256: sha256 || null,
          resume: resume || false,
        });
        if (window._hfTokenResolve) {
          window._hfTokenResolve(result);
          window._hfTokenResolve = null;
          window._hfTokenReject = null;
        }
      } catch (retryErr) {
        if (window._hfTokenReject) {
          window._hfTokenReject(retryErr);
          window._hfTokenResolve = null;
          window._hfTokenReject = null;
        }
      }
    }
  } catch (err) {
    state.buildLog.push(`Failed to save HF token: ${err}`);
    renderBuildLog();
  }
}

function cancelHfTokenPrompt() {
  state.hfTokenPromptVisible = false;
  state.hfTokenPendingRetry = null;
  renderCurrentStep();
  if (window._hfTokenReject) {
    window._hfTokenReject('User cancelled');
    window._hfTokenResolve = null;
    window._hfTokenReject = null;
  }
}

// Check if file exists and prompt user for action
// Returns: 'skip' (use existing), 'download' (fresh download), 'resume' (continue partial), or throws on cancel
async function checkFileExistsWithPrompt(path, expectedSize, sha256) {
  const fileInfo = await invoke('get_file_info', {
    path,
    expectedSize,
    expectedSha256: sha256,
    verifyChecksum: true,
  });

  const filename = path.split('/').pop();

  if (!fileInfo.exists) {
    state.buildLog.push(`${filename}: not found, will download`);
    renderBuildLog();
    return 'download';
  }

  // File is partial - offer resume/delete/cancel
  if (fileInfo.isPartial) {
    const pct = ((fileInfo.size / fileInfo.expectedSize) * 100).toFixed(1);
    state.buildLog.push(`${filename}: partial download found (${formatBytes(fileInfo.size)} / ${formatBytes(fileInfo.expectedSize)}, ${pct}%)`);
    renderBuildLog();
    return new Promise((resolve) => {
      state.fileExistsPrompt = {
        filename,
        path,
        isPartial: true,
        currentSize: fileInfo.size,
        expectedSize: fileInfo.expectedSize,
        resolve,
      };
      renderCurrentStep();
    });
  }

  // File is complete - check if checksum is valid
  if (fileInfo.isComplete) {
    if (fileInfo.checksumValid === false) {
      // Checksum mismatch - offer re-download or use anyway
      state.buildLog.push(`${filename}: checksum mismatch (file may be corrupted)`);
      renderBuildLog();
      return new Promise((resolve) => {
        state.fileExistsPrompt = {
          filename,
          path,
          checksumMismatch: true,
          resolve,
        };
        renderCurrentStep();
      });
    }
    // File is complete and valid (or no checksum to verify)
    const checksumVerified = fileInfo.checksumValid === true;
    const validationMsg = checksumVerified ? 'checksum verified' : 'size verified';
    state.buildLog.push(`${filename}: found and ${validationMsg}`);
    renderBuildLog();
    return new Promise((resolve) => {
      state.fileExistsPrompt = {
        filename,
        path,
        isComplete: true,
        checksumVerified,
        resolve,
      };
      renderCurrentStep();
    });
  }

  // File exists but we don't know expected size - ask user
  state.buildLog.push(`${filename}: found (${formatBytes(fileInfo.size)})`);
  renderBuildLog();
  return new Promise((resolve) => {
    state.fileExistsPrompt = { filename, path, resolve };
    renderCurrentStep();
  });
}

function handleFileExistsChoice(choice) {
  if (state.fileExistsPrompt?.resolve) {
    state.fileExistsPrompt.resolve(choice);
  }
  state.fileExistsPrompt = null;
  renderCurrentStep();
}

async function deletePartialFile(path) {
  try {
    await invoke('delete_file', { path });
    state.buildLog.push(`Deleted partial file: ${path}`);
    renderBuildLog();
  } catch (err) {
    state.buildLog.push(`Failed to delete file: ${err}`);
    renderBuildLog();
  }
}

async function selectOutputPath() {
  try {
    // Use current outputPath as default
    const defaultPath = state.outputPath || '/output.llamafile';
    const path = await invoke('select_output_path', { defaultPath });
    if (path) {
      // Update Rust build state
      const buildState = await invoke('set_output_path', { path });
      syncFromBuildState(buildState);
      renderCurrentStep();
    }
  } catch (err) { console.error('Failed to select output path:', err); }
}

async function startBuild() {
  state.isBuilding = true;
  state.buildLog = [];
  state.downloadProgress = null;
  renderCurrentStep();

  try {
    // Step 1: Download llamafile runtime (if from registry)
    let llamafilePath = state.resolvedLlamafilePath;

    if (state.llamafileSource === 'registry' && state.selectedRelease) {
      state.buildProgress = { stage: 'downloading_runtime', progress_pct: 0, message: 'Downloading llamafile runtime...' };
      renderCurrentStep();

      const release = state.selectedRelease;
      const cacheDir = await invoke('get_llamafile_cache_dir');
      const filename = getFilenameFromUrl(release.download_url);
      const cachePath = `${cacheDir}/${filename}`;

      const action = await checkFileExistsWithPrompt(cachePath, release.size_bytes, release.sha256);

      if (action === 'cancel') {
        throw new Error('Download cancelled by user');
      } else if (action === 'download' || action === 'resume') {
        const isResume = action === 'resume';
        state.buildLog.push(`${isResume ? 'Resuming' : 'Downloading'} llamafile ${release.version}...`);
        renderBuildLog();
        state.currentDownloadPath = cachePath;
        try {
          llamafilePath = await downloadWithAuth(release.download_url, cachePath, release.size_bytes, release.sha256, isResume);
          state.currentDownloadPath = null;
          state.downloadProgress = null;
          const verifyMsg = release.sha256 ? 'downloaded and checksum verified' : 'downloaded';
          state.buildLog.push(`${filename}: ${verifyMsg}`);
          renderBuildLog();
        } catch (err) {
          state.currentDownloadPath = null;
          state.downloadProgress = null;
          state.buildLog.push(`Download failed: ${err}`);
          renderBuildLog();
          throw err;
        }
      } else {
        llamafilePath = cachePath;
        state.buildLog.push(`Using existing: ${cachePath}`);
        renderBuildLog();
      }
    } else if (state.llamafileSource === 'custom' && llamafilePath) {
      // Custom llamafile binary - just log the path
      const filename = llamafilePath.split('/').pop();
      state.buildLog.push(`${filename}: using custom binary`);
      renderBuildLog();
    }

    state.buildProgress = { stage: 'downloading_runtime', progress_pct: 50 };
    renderCurrentStep();

    // Step 2: Download model files (if from registry)
    const modelFiles = []; // { role, path }

    if (state.modelSource === 'registry' && state.selectedModel) {
      state.buildProgress = { stage: 'downloading_model', progress_pct: 0, message: 'Downloading model files...' };
      renderCurrentStep();

      for (const file of state.selectedModel.files) {
        const sel = state.modelFileSelections[file.role];
        if (!sel) continue;

        if (sel.source === 'custom' && sel.customPath) {
          // User-provided file - just log the path
          modelFiles.push({ role: file.role, path: sel.customPath });
          const filename = sel.customPath.split('/').pop();
          state.buildLog.push(`${filename}: using custom file`);
          renderBuildLog();
        } else {
          // Download from registry
          const quant = file.quantizations[sel.quantizationIdx] || file.quantizations[0];
          if (!quant.download_url) {
            state.buildLog.push(`No download URL for ${file.role} ${quant.name}`);
            renderBuildLog();
            continue;
          }

          // Build destination path: modelDownloadDir/model-id/filename
          const filename = quant.download_url.split('/').pop();
          const modelDir = `${state.modelDownloadDir}/${state.selectedModel.id}`;
          const destPath = `${modelDir}/${filename}`;

          const action = await checkFileExistsWithPrompt(destPath, quant.size_bytes, quant.sha256);

          if (action === 'cancel') {
            throw new Error('Download cancelled by user');
          } else if (action === 'download' || action === 'resume') {
            const isResume = action === 'resume';
            state.buildLog.push(`${isResume ? 'Resuming' : 'Downloading'} ${file.label || file.role} (${quant.name})...`);
            renderBuildLog();
            state.currentDownloadPath = destPath;
            try {
              const path = await downloadWithAuth(quant.download_url, destPath, quant.size_bytes, quant.sha256, isResume);
              state.currentDownloadPath = null;
              state.downloadProgress = null;
              modelFiles.push({ role: file.role, path });
              const verifyMsg = quant.sha256 ? 'downloaded and checksum verified' : 'downloaded';
              state.buildLog.push(`${filename}: ${verifyMsg}`);
              renderBuildLog();
            } catch (err) {
              state.currentDownloadPath = null;
              state.downloadProgress = null;
              state.buildLog.push(`Download failed: ${err}`);
              renderBuildLog();
              throw err;
            }
          } else {
            modelFiles.push({ role: file.role, path: destPath });
            state.buildLog.push(`Using existing: ${destPath}`);
            renderBuildLog();
          }
        }
      }
    } else if (state.modelSource === 'custom') {
      // Custom model files - just log the paths
      for (const f of state.customModelFiles) {
        modelFiles.push({ role: f.role, path: f.path });
        const filename = f.path.split('/').pop();
        state.buildLog.push(`${filename}: using custom file`);
        renderBuildLog();
      }
    }

    state.buildProgress = { stage: 'downloading_model', progress_pct: 60 };
    renderCurrentStep();

    // Step 3: Generate and write .args configuration
    state.buildProgress = { stage: 'writing_config', progress_pct: 70, message: 'Writing configuration...' };
    renderCurrentStep();

    // Get model and mmproj filenames for .args generation
    const weightsFile = modelFiles.find(f => f.role === 'weights');
    const mmprojFile = modelFiles.find(f => f.role === 'mmproj');

    if (!weightsFile) {
      throw new Error('No model weights file found');
    }

    const modelFilename = weightsFile.path.split('/').pop();
    const mmprojFilename = mmprojFile ? mmprojFile.path.split('/').pop() : null;

    // Prepare config values for .args generation
    // Add gpu_mode: 'DISABLE' for CPU-only mode to prevent Metal from being used
    const configValuesForBuild = { ...state.configValues };
    if (state.computeMode === 'cpu') {
      configValuesForBuild.gpu_mode = 'DISABLE';
    }

    // Generate .args content
    const argsContent = await invoke('generate_args_content', {
      configValues: configValuesForBuild,
      modelFilename,
      mmprojFilename,
    });

    // Write .args to temp file
    const argsPath = await invoke('write_temp_file', {
      content: argsContent,
      filename: '.args',
    });

    state.buildLog.push('Generated .args configuration');
    state.buildLog.push(argsContent.split('\n').map(l => `  ${l}`).join('\n'));
    renderBuildLog();

    // Step 4: Download and prepare GPU libraries
    state.buildProgress = { stage: 'bundling_gpu', progress_pct: 80, message: 'Preparing GPU libraries...' };
    renderCurrentStep();
    state.buildLog.push(`Compute mode: ${state.computeMode}`);
    renderBuildLog();

    // Track all GPU lib paths for zipalign (can have multiple per lib type for different platforms)
    const gpuLibPaths = [];

    if (state.computeMode === 'gpu') {
      const gpuTargets = [];
      if (state.gpuOptions.metal) gpuTargets.push('Metal (built-in)');

      // Check if GPU libs are available for selected release
      const availableGpuLibs = getGpuLibsForRelease(state.selectedRelease);
      const version = state.selectedRelease?.version || 'unknown';
      const cacheDir = await invoke('get_gpu_lib_cache_dir', { version });

      // Helper to download a GPU lib for a specific platform
      async function downloadGpuLib(libType, platform, libInfo, progressPct) {
        const platformSuffix = platform === 'windows' ? '.dll' : '.so';
        const filename = `ggml-${libType}${platformSuffix}`;
        const cachePath = `${cacheDir}/${filename}`;

        state.buildProgress = { stage: 'bundling_gpu', progress_pct: progressPct, message: `Downloading ${libType.toUpperCase()} library (${platform})...` };
        renderCurrentStep();

        const action = await checkFileExistsWithPrompt(
          cachePath,
          libInfo.size_bytes,
          libInfo.sha256
        );

        if (action === 'cancel') {
          throw new Error(`${libType.toUpperCase()} library download cancelled`);
        } else if (action === 'download' || action === 'resume') {
          const isResume = action === 'resume';
          state.buildLog.push(`${isResume ? 'Resuming' : 'Downloading'} ${libType.toUpperCase()} library (${platform})...`);
          renderBuildLog();
          state.currentDownloadPath = cachePath;
          try {
            const downloadedPath = await downloadWithAuth(
              libInfo.download_url,
              cachePath,
              libInfo.size_bytes,
              libInfo.sha256,
              isResume
            );
            state.currentDownloadPath = null;
            state.downloadProgress = null;
            state.buildLog.push(`${libType.toUpperCase()} (${platform}): downloaded`);
            renderBuildLog();
            return downloadedPath;
          } catch (err) {
            state.currentDownloadPath = null;
            state.downloadProgress = null;
            state.buildLog.push(`${libType.toUpperCase()} (${platform}) download failed: ${err}`);
            renderBuildLog();
            const continueWithout = confirm(`${libType.toUpperCase()} library (${platform}) download failed: ${err}\n\nContinue without this library?`);
            if (!continueWithout) {
              throw new Error(`${libType.toUpperCase()} library download failed: ${err}`);
            }
            return null;
          }
        } else {
          state.buildLog.push(`${libType.toUpperCase()} (${platform}): using cached`);
          renderBuildLog();
          return cachePath;
        }
      }

      // Download CUDA libraries for all available platforms
      if (state.gpuOptions.cuda && availableGpuLibs.cuda) {
        let progressPct = 82;
        for (const platform of availableGpuLibs.cuda.platforms) {
          const libInfo = availableGpuLibs.cuda.libs[platform];
          if (libInfo) {
            const path = await downloadGpuLib('cuda', platform, libInfo, progressPct);
            if (path) gpuLibPaths.push(path);
            progressPct += 1;
          }
        }
        if (gpuLibPaths.length > 0) gpuTargets.push('CUDA');
      } else if (state.gpuOptions.cuda && !availableGpuLibs.cuda) {
        state.buildLog.push('CUDA: not available for this llamafile version');
        renderBuildLog();
      }

      // Download ROCm libraries for all available platforms
      if (state.gpuOptions.rocm && availableGpuLibs.rocm) {
        let progressPct = 84;
        for (const platform of availableGpuLibs.rocm.platforms) {
          const libInfo = availableGpuLibs.rocm.libs[platform];
          if (libInfo) {
            const path = await downloadGpuLib('rocm', platform, libInfo, progressPct);
            if (path) gpuLibPaths.push(path);
            progressPct += 1;
          }
        }
        gpuTargets.push('ROCm');
      } else if (state.gpuOptions.rocm && !availableGpuLibs.rocm) {
        state.buildLog.push('ROCm: not available for this llamafile version/platform');
        renderBuildLog();
      }

      state.buildLog.push(`GPU targets: ${gpuTargets.join(', ') || 'none'}`);
      renderBuildLog();
    } else {
      state.buildLog.push('GPU acceleration disabled (--gpu DISABLE)');
      renderBuildLog();
    }

    // Step 5: Create executable
    state.buildProgress = { stage: 'creating_executable', progress_pct: 85, message: 'Creating executable...' };
    renderCurrentStep();

    // Copy llamafile to output path
    state.buildLog.push(`Copying llamafile to ${state.outputPath}...`);
    renderBuildLog();
    await invoke('copy_file', { src: llamafilePath, dest: state.outputPath });

    // Make it executable
    await invoke('make_executable', { path: state.outputPath });

    // Build zipalign args: -j0 <output> <.args> <weights> [<mmproj>]
    const zipalignArgs = ['-j0', state.outputPath, argsPath];

    // Add model weights
    zipalignArgs.push(weightsFile.path);
    state.buildLog.push(`Adding weights: ${modelFilename}`);
    renderBuildLog();

    // Add mmproj if present
    if (mmprojFile) {
      zipalignArgs.push(mmprojFile.path);
      state.buildLog.push(`Adding mmproj: ${mmprojFilename}`);
      renderBuildLog();
    }

    // Add GPU libraries if downloaded
    for (const gpuLibPath of gpuLibPaths) {
      zipalignArgs.push(gpuLibPath);
      const libName = gpuLibPath.split('/').pop();
      state.buildLog.push(`Adding GPU library: ${libName}`);
      renderBuildLog();
    }

    // Run zipalign to bundle everything
    state.buildProgress = { stage: 'creating_executable', progress_pct: 95, message: 'Running zipalign...' };
    renderCurrentStep();

    const exitCode = await invoke('run_zipalign', { args: zipalignArgs });

    if (exitCode !== 0) {
      throw new Error(`zipalign failed with exit code ${exitCode}`);
    }

    // Done
    state.buildProgress = { stage: 'complete', progress_pct: 100, message: 'Build complete!' };
    state.buildLog.push(`Build complete: ${state.outputPath}`);
    renderBuildLog();

  } catch (err) {
    // Don't update state if build was already cancelled (cancelBuild already cleared state)
    if (!state.isBuilding) return;

    state.buildProgress = { stage: 'error', progress_pct: 0, message: `Error: ${err}` };
    state.buildLog.push(`Build failed: ${err}`);
    renderBuildLog();
  }

  state.isBuilding = false;
  state.downloadProgress = null;
  state.currentDownloadPath = null;
  renderCurrentStep();
}

function cancelBuild() {
  // Show confirmation modal
  state.cancelConfirmVisible = true;
  renderCurrentStep();
}

async function confirmCancelBuild() {
  state.cancelConfirmVisible = false;

  // Cancel any ongoing download
  try {
    await invoke('cancel_download');
  } catch (err) {
    // Ignore errors - download might not be active
  }

  // Delete partial file if there was a download in progress
  if (state.currentDownloadPath) {
    try {
      await invoke('delete_file', { path: state.currentDownloadPath });
    } catch (err) {
      // Ignore errors - file might not exist
    }
  }

  // Fully reset build state
  state.isBuilding = false;
  state.buildProgress = null;
  state.buildLog = [];
  state.downloadProgress = null;
  state.fileExistsPrompt = null;
  state.currentDownloadPath = null;
  renderCurrentStep();
}

function dismissCancelConfirm() {
  state.cancelConfirmVisible = false;
  renderCurrentStep();
}
async function openOutputFolder() {
  const folder = state.outputPath.substring(0, state.outputPath.lastIndexOf('/'));
  try {
    await invoke('open_url', { url: folder });
  } catch (err) {
    console.error('Failed to open folder:', err);
  }
}

// ============================================================================
// Navigation
// ============================================================================

async function goToStep(step) {
  if (step > state.maxVisitedStep) return;
  state.currentStep = step;
  // Refresh .args content when entering Build step
  if (step === 6) await refreshArgsContent();
  renderSidebar();
  renderCurrentStep();
}

async function nextStep() {
  if (state.currentStep < STEPS.length) {
    // Skip steps 4, 5 (Compute, Agent) - go from Configuration to Build
    if (state.currentStep === 4) {
      state.currentStep = 6;
    } else {
      state.currentStep++;
    }
    state.maxVisitedStep = Math.max(state.maxVisitedStep, state.currentStep);
    // Refresh .args content when entering Build step
    if (state.currentStep === 6) await refreshArgsContent();
    renderSidebar();
    renderCurrentStep();
  }
}

function prevStep() {
  if (state.currentStep > 1) {
    // Reset build state when leaving the build step
    if (state.currentStep === 6) {
      state.buildProgress = null;
      state.buildLog = [];
      state.downloadProgress = null;
      state.currentDownloadPath = null;
      // Skip steps 4, 5 - go back from Build to Configuration
      state.currentStep = 3;
    } else {
      state.currentStep--;
    }
    renderSidebar();
    renderCurrentStep();
  }
}

// ============================================================================
// Utilities
// ============================================================================

function formatBytes(bytes) {
  if (bytes >= 1e9) return (bytes / 1e9).toFixed(2) + ' GB';
  if (bytes >= 1e6) return (bytes / 1e6).toFixed(1) + ' MB';
  if (bytes >= 1e3) return (bytes / 1e3).toFixed(0) + ' KB';
  return bytes + ' B';
}

function getFileName(path) {
  if (!path) return 'N/A';
  return path.split('/').pop().split('\\').pop();
}

function getSelectedGgufFilename() {
  // Get GGUF filename from current model selection
  if (state.modelSource === 'custom' && state.customModelFiles.length > 0) {
    const weightsFile = state.customModelFiles.find(f => f.role === 'weights');
    if (weightsFile?.path) {
      return weightsFile.path.split('/').pop();
    }
  }

  if (state.selectedModel && state.modelFileSelections) {
    const sel = state.modelFileSelections['weights'];
    if (sel?.source === 'custom' && sel.customPath) {
      return sel.customPath.split('/').pop();
    }
    const weightsFile = state.selectedModel.files?.find(f => f.role === 'weights');
    const quant = weightsFile?.quantizations[sel?.quantizationIdx || 0];
    if (quant?.download_url) {
      return getFilenameFromUrl(quant.download_url);
    }
  }

  return null;
}

function getFilenameFromUrl(url) {
  if (!url) return 'file';
  try {
    const parsed = new URL(url);
    return parsed.pathname.split('/').pop() || 'file';
  } catch {
    return url.split('/').pop() || 'file';
  }
}

// Get current platform for GPU library selection
function getCurrentPlatform() {
  // In Tauri, we can detect via navigator.platform or system info
  // For now, use a simple heuristic that works in browser and Tauri
  const platform = navigator.platform?.toLowerCase() || '';
  if (platform.includes('win')) return 'windows';
  if (platform.includes('linux')) return 'linux';
  if (platform.includes('mac')) return 'darwin';
  // Fallback based on userAgent
  const ua = navigator.userAgent?.toLowerCase() || '';
  if (ua.includes('windows')) return 'windows';
  if (ua.includes('linux')) return 'linux';
  return 'darwin'; // Default to macOS
}

// Get GPU library info for a release (all platforms, for cross-platform bundling)
function getGpuLibsForRelease(release) {
  if (!release?.gpu_libs) return { cuda: null, rocm: null };

  const cuda = release.gpu_libs.cuda;
  const rocm = release.gpu_libs.rocm;

  // Aggregate CUDA libs across platforms
  let cudaInfo = null;
  if (cuda && (cuda.linux || cuda.windows)) {
    const platforms = [];
    let totalSize = 0;
    if (cuda.linux) {
      platforms.push('linux');
      totalSize += cuda.linux.size_bytes;
    }
    if (cuda.windows) {
      platforms.push('windows');
      totalSize += cuda.windows.size_bytes;
    }
    cudaInfo = { platforms, totalSize, libs: cuda };
  }

  // Aggregate ROCm libs (Linux only)
  let rocmInfo = null;
  if (rocm && rocm.linux) {
    rocmInfo = {
      platforms: ['linux'],
      totalSize: rocm.linux.size_bytes,
      libs: rocm,
    };
  }

  return { cuda: cudaInfo, rocm: rocmInfo };
}

async function openExternalLink(event, url) {
  event.preventDefault();
  event.stopPropagation();
  try {
    await invoke('open_url', { url });
  } catch (err) {
    console.error('Failed to open URL:', err);
    window.open(url, '_blank');
  }
}

// ============================================================================
// Event Delegation System
// ============================================================================

/**
 * Extract data attributes as parameters
 * Converts data-kebab-case to camelCase
 */
function extractParams(element) {
  const params = {};
  for (const attr of element.attributes) {
    if (attr.name.startsWith('data-') && 
        attr.name !== 'data-action' && 
        attr.name !== 'data-stop-propagation') {
      // Convert data-model-id to modelId, data-group-name to groupName, etc.
      const key = attr.name
        .replace('data-', '')
        .replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      params[key] = attr.value;
    }
  }
  return params;
}

/**
 * Global click handler for all click events
 * Uses data-action attribute to determine which function to call
 */
async function handleGlobalClick(event) {
  const target = event.target.closest('[data-action]');
  if (!target) return;
  
  const action = target.getAttribute('data-action');
  const stopProp = target.getAttribute('data-stop-propagation') === 'true';
  
  if (stopProp) {
    event.stopPropagation();
  }
  
  event.preventDefault();
  
  const params = extractParams(target);
  
  try {
    switch (action) {
      // Navigation
      case 'goToStep':
        await goToStep(parseInt(params.step));
        break;
      case 'nextStep':
        await nextStep();
        break;
      case 'prevStep':
        prevStep();
        break;
      
      // Version Selection
      case 'selectVersionGroup':
        await selectVersionGroup(params.groupName);
        break;
      case 'selectSpecificRelease':
        await selectSpecificRelease(params.groupName, params.version);
        break;
      case 'toggleVersionGroup':
        toggleVersionGroup(params.groupName);
        break;
      case 'selectCustomLlamafile':
        await selectCustomLlamafile();
        break;
      case 'clearCustomLlamafile':
        await clearCustomLlamafile();
        break;
      
      // Model Selection
      case 'selectModel':
        await selectModel(params.modelId);
        break;
      case 'filterModels':
        filterModels(params.filter);
        break;
      case 'selectDownloadDir':
        await selectDownloadDir();
        break;
      case 'toggleModelFileExpand':
        toggleModelFileExpand(params.role);
        break;
      case 'selectFileQuantization':
        await selectFileQuantization(params.role, parseInt(params.index));
        break;
      case 'selectCustomFileForRole':
        await selectCustomFileForRole(params.role);
        break;
      case 'clearCustomFileForRole':
        await clearCustomFileForRole(params.role);
        break;
      case 'selectCustomModel':
        await selectCustomModel();
        break;
      case 'addCompanionFile':
        await addCompanionFile();
        break;
      case 'removeCompanionFile':
        await removeCompanionFile(parseInt(params.index));
        break;
      case 'clearCustomModel':
        await clearCustomModel();
        break;
      
      // Configuration
      case 'toggleConfig':
        await toggleConfig(params.configKey);
        break;
      
      // Compute
      case 'selectComputeMode':
        selectComputeMode(params.mode);
        break;
      case 'toggleGpuOption':
        toggleGpuOption(params.option);
        break;
      
      // Agent
      case 'toggleAgent':
        toggleAgent();
        break;
      case 'toggleTool':
        toggleTool(params.toolId);
        break;
      
      // Build
      case 'startBuild':
        await startBuild();
        break;
      case 'cancelBuild':
        cancelBuild();
        break;
      case 'resetBuild':
        resetBuild();
        break;
      case 'openOutputFolder':
        await openOutputFolder();
        break;
      case 'refreshArgsContent':
        await refreshArgsContent();
        renderCurrentStep();
        break;
      case 'selectOutputPath':
        await selectOutputPath();
        break;
      
      // Modals
      case 'submitHfToken':
        {
          const input = document.getElementById('hf-token-input');
          await submitHfToken(input ? input.value : '');
        }
        break;
      case 'cancelHfTokenPrompt':
        cancelHfTokenPrompt();
        break;
      case 'handleFileExistsChoice':
        handleFileExistsChoice(params.choice);
        break;
      case 'handleShaVerifyChoice':
        await handleShaVerifyChoice(params.choice);
        break;
      case 'confirmCancelBuild':
        await confirmCancelBuild();
        break;
      case 'dismissCancelConfirm':
        dismissCancelConfirm();
        break;
      
      // External links
      case 'openExternalLink':
        await openExternalLink(event, params.url);
        break;
      
      default:
        console.warn('Unknown action:', action, params);
    }
  } catch (err) {
    console.error('Error handling action:', action, err);
  }
}

/**
 * Global input handler for oninput events
 */
function handleGlobalInput(event) {
  const target = event.target.closest('[data-action]');
  if (!target) return;
  
  const action = target.getAttribute('data-action');
  const params = extractParams(target);
  
  try {
    switch (action) {
      case 'searchModels':
        searchModels(event.target.value);
        break;
      case 'updateSlider':
        {
          const isInteger = params.isInteger === 'true';
          updateSlider(params.configKey, event.target.value, isInteger);
        }
        break;
      case 'updateArgsContent':
        updateArgsContent(event.target.value);
        break;
      default:
        // Ignore unknown input actions
        break;
    }
  } catch (err) {
    console.error('Error handling input:', action, err);
  }
}

/**
 * Global change handler for onchange events
 */
function handleGlobalChange(event) {
  const target = event.target.closest('[data-action]');
  if (!target) return;
  
  const action = target.getAttribute('data-action');
  const params = extractParams(target);
  
  try {
    switch (action) {
      case 'updateConfig':
        {
          const valueType = params.valueType;
          const value = valueType === 'number' ? parseInt(event.target.value) : event.target.value;
          updateConfig(params.configKey, value);
        }
        break;
      case 'updateSystemPrompt':
        updateSystemPrompt(event.target.value);
        break;
      case 'updateArgsContent':
        updateArgsContent(event.target.value);
        break;
      default:
        // Ignore unknown change actions
        break;
    }
  } catch (err) {
    console.error('Error handling change:', action, err);
  }
}

/**
 * Global keydown handler for special keys (e.g., Enter in HF token input)
 */
function handleGlobalKeydown(event) {
  if (event.key !== 'Enter') return;
  
  const target = event.target.closest('[data-action]');
  if (!target) return;
  
  const action = target.getAttribute('data-action');
  
  if (action === 'submitHfToken') {
    event.preventDefault();
    submitHfToken(event.target.value);
  }
}

document.addEventListener('DOMContentLoaded', init);
