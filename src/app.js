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
  buildProgress: null,
  isBuilding: false,
  buildLog: [],
  downloadProgress: null, // { filename, bytes_downloaded, bytes_total, percentage }
  hfTokenPromptVisible: false,
  hfTokenPendingRetry: null, // { url, destPath, sha256 } - retry after token entry
  fileExistsPrompt: null, // { filename, path, resolve } - prompt when file exists
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

const invoke = window.__TAURI__?.core?.invoke || (async (cmd, args) => {
  console.log(`[Mock] invoke(${cmd})`, args);
  switch (cmd) {
    case 'get_llamafile_versions':
      return getMockVersions();
    case 'get_models':
      return getMockModels();
    case 'get_config_schema':
      return getMockConfigSchema();
    case 'search_models':
      return getMockModels().filter(m =>
        m.name.toLowerCase().includes((args?.query || '').toLowerCase())
      );
    case 'get_default_output_path':
      return `~/Desktop/${args?.model_name || 'model'}.llamafile`;
    case 'get_system_info':
      return { os: 'macos', arch: 'aarch64', cpu_cores: 8, memory_gb: 16, has_metal: true };
    case 'select_llamafile_path':
      return '/Users/me/builds/llamafile';
    case 'get_llamafile_cache_dir':
      return `~/.llamafile/v/${args?.version}/bin`;
    case 'get_model_cache_dir':
      return '~/.llamafile/models';
    case 'select_model_file_path':
      return '/Users/me/models/custom-model.gguf';
    case 'select_model_download_dir':
      return '/Users/me/models';
    case 'run_zipalign':
      return 1;
    case 'download_file':
      // Mock: simulate download success
      console.log(`[Mock] download_file: ${args?.url} -> ${args?.dest_path}`);
      return args?.dest_path;
    case 'file_exists':
      // Mock: files don't exist (trigger download)
      return false;
    case 'get_hf_token':
      return null;
    case 'set_hf_token':
      return null;
    case 'clear_hf_token':
      return null;
    default:
      return null;
  }
});

const listen = window.__TAURI__?.event?.listen || ((event, callback) => {
  console.log(`[Mock] listen(${event})`);
  return () => {};
});

// ============================================================================
// Mock Data
// ============================================================================

function getMockVersions() {
  return [
    {
      name: 'llamafile 0.9',
      tag: 'latest',
      description: 'Latest release — improved Metal support, new quantization options, [release notes](https://github.com/Mozilla-Ocho/llamafile/releases/tag/0.9.4)',
      releases: [
        { version: '0.9.4', description: 'Metal performance fixes, improved prompt caching, [release notes](https://github.com/Mozilla-Ocho/llamafile/releases/tag/0.9.4)', release_date: '2024-12-20', download_url: 'https://github.com/Mozilla-Ocho/llamafile/releases/download/0.9.4/llamafile-0.9.4', size_bytes: 5300000, sha256: 'a1b2c3d4e5f6...' },
        { version: '0.9.2', description: 'New quantization options, faster tokenizer, [release notes](https://github.com/Mozilla-Ocho/llamafile/releases/tag/0.9.2)', release_date: '2024-12-15', download_url: 'https://github.com/Mozilla-Ocho/llamafile/releases/download/0.9.2/llamafile-0.9.2', size_bytes: 5200000, sha256: 'b2c3d4e5f6a1...' },
        { version: '0.9.0', description: 'Initial 0.9 release with Metal support overhaul, [release notes](https://github.com/Mozilla-Ocho/llamafile/releases/tag/0.9.0)', release_date: '2024-12-01', download_url: 'https://github.com/Mozilla-Ocho/llamafile/releases/download/0.9.0/llamafile-0.9.0', size_bytes: 5100000, sha256: 'c3d4e5f6a1b2...' },
      ],
    },
    {
      name: 'llamafile 0.8',
      tag: 'stable',
      description: 'Stable release — CUDA 12 support, multi-GPU, flash attention, **recommended for production**',
      releases: [
        { version: '0.8.16', description: 'Speculative decoding, bug fixes, [release notes](https://github.com/Mozilla-Ocho/llamafile/releases/tag/0.8.16)', release_date: '2024-10-20', download_url: 'https://github.com/Mozilla-Ocho/llamafile/releases/download/0.8.16/llamafile-0.8.16', size_bytes: 5100000, sha256: 'd4e5f6a1b2c3...' },
        { version: '0.8.14', description: 'Multi-GPU improvements, [release notes](https://github.com/Mozilla-Ocho/llamafile/releases/tag/0.8.14)', release_date: '2024-10-05', download_url: 'https://github.com/Mozilla-Ocho/llamafile/releases/download/0.8.14/llamafile-0.8.14', size_bytes: 5050000, sha256: 'e5f6a1b2c3d4...' },
      ],
    },
    {
      name: 'llamafile 0.7',
      tag: 'legacy',
      description: 'Legacy — broad compatibility, well-tested with older model architectures',
      releases: [
        { version: '0.7.6', release_date: '2024-06-01', download_url: 'https://github.com/Mozilla-Ocho/llamafile/releases/download/0.7.6/llamafile-0.7.6', size_bytes: 4800000, sha256: 'f6a1b2c3d4e5...' },
      ],
    },
  ];
}

function getMockModels() {
  return [
    {
      id: 'llama-3.1-8b-instruct',
      name: 'Llama 3.1 8B Instruct',
      organization: 'Meta',
      parameters: '8B',
      param_count: 8000000000,
      description: "Meta's latest instruction-tuned model with strong reasoning capabilities",
      license: 'Llama 3.1 Community License',
      icon: '\ud83e\udd99',
      huggingface_url: 'https://huggingface.co/meta-llama/Meta-Llama-3.1-8B-Instruct',
      tags: ['chat'],
      context_lengths: [2048, 4096, 8192, 16384, 32768, 65536, 131072],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 4900000000, download_url: '', sha256: null },
          { name: 'Q5_K_M', size_bytes: 5700000000, download_url: '', sha256: null },
          { name: 'Q8_0', size_bytes: 8500000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'mistral-7b-instruct-v0.3',
      name: 'Mistral 7B Instruct v0.3',
      organization: 'Mistral AI',
      parameters: '7B',
      param_count: 7000000000,
      description: 'Fast and capable instruction-following model',
      license: 'Apache 2.0',
      icon: '\ud83c\udf0a',
      huggingface_url: 'https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3',
      tags: ['chat'],
      context_lengths: [2048, 4096, 8192, 16384, 32768],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 4400000000, download_url: '', sha256: null },
          { name: 'Q5_K_M', size_bytes: 5100000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'gemma-2-9b-instruct',
      name: 'Gemma 2 9B Instruct',
      organization: 'Google',
      parameters: '9B',
      param_count: 9000000000,
      description: "Google's efficient and capable instruction model",
      license: 'Gemma License',
      icon: '\ud83d\udc8e',
      huggingface_url: 'https://huggingface.co/google/gemma-2-9b-it',
      tags: ['chat'],
      context_lengths: [2048, 4096, 8192],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 5500000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'phi-3.5-mini-instruct',
      name: 'Phi-3.5 Mini Instruct',
      organization: 'Microsoft',
      parameters: '3.8B',
      param_count: 3800000000,
      description: 'Compact yet capable model, great for constrained environments',
      license: 'MIT',
      icon: '\u26a1',
      huggingface_url: 'https://huggingface.co/microsoft/Phi-3.5-mini-instruct',
      tags: ['chat'],
      context_lengths: [2048, 4096, 8192, 16384, 32768, 65536, 131072],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 2400000000, download_url: '', sha256: null },
          { name: 'Q5_K_S', size_bytes: 2800000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'qwen2.5-coder-7b-instruct',
      name: 'Qwen2.5 Coder 7B Instruct',
      organization: 'Alibaba',
      parameters: '7B',
      param_count: 7000000000,
      description: 'Specialized coding model with strong code generation',
      license: 'Apache 2.0',
      icon: '\ud83e\udde0',
      huggingface_url: 'https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct',
      tags: ['code'],
      context_lengths: [2048, 4096, 8192, 16384, 32768],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 4700000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'deepseek-r1-distill-qwen-7b',
      name: 'DeepSeek-R1 Distill Qwen 7B',
      organization: 'DeepSeek',
      parameters: '7B',
      param_count: 7000000000,
      description: 'Reasoning-focused model distilled from DeepSeek-R1',
      license: 'MIT',
      icon: '\ud83d\udd2e',
      huggingface_url: 'https://huggingface.co/deepseek-ai/DeepSeek-R1-Distill-Qwen-7B',
      tags: ['chat'],
      context_lengths: [2048, 4096, 8192, 16384, 32768, 65536],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 4700000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'nomic-embed-text-v1.5',
      name: 'Nomic Embed Text v1.5',
      organization: 'Nomic AI',
      parameters: '137M',
      param_count: 137000000,
      description: 'High-quality text embeddings for semantic search',
      license: 'Apache 2.0',
      icon: '\ud83d\udd17',
      huggingface_url: 'https://huggingface.co/nomic-ai/nomic-embed-text-v1.5',
      tags: ['embedding'],
      context_lengths: [512, 1024, 2048, 8192],
      default_context: 2048,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'F16', size_bytes: 274000000, download_url: '', sha256: null },
        ]},
      ],
    },
    {
      id: 'llava-v1.6-mistral-7b',
      name: 'LLaVA v1.6 Mistral 7B',
      organization: 'Haotian Liu',
      parameters: '7B',
      param_count: 7000000000,
      description: 'Multimodal vision-language model — understands both text and images',
      license: 'Apache 2.0',
      icon: '\ud83d\udc41\ufe0f',
      huggingface_url: 'https://huggingface.co/cjpais/llava-v1.6-mistral-7b-gguf',
      tags: ['vision', 'chat'],
      context_lengths: [2048, 4096, 8192],
      default_context: 4096,
      files: [
        { role: 'weights', label: 'Model Weights', quantizations: [
          { name: 'Q4_K_M', size_bytes: 4370000000, download_url: '', sha256: null },
          { name: 'Q5_K_M', size_bytes: 5130000000, download_url: '', sha256: null },
        ]},
        { role: 'mmproj', label: 'Vision Adapter (mmproj)', quantizations: [
          { name: 'F16', size_bytes: 624000000, download_url: '', sha256: null },
        ]},
      ],
    },
  ];
}

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

function getMockConfigSchema() {
  return [
    { type: 'section', label: 'Server' },
    { type: 'text', key: 'host', label: 'Listen Host', description: 'Network interface', default: '127.0.0.1', value_type: 'string', flag: '--host' },
    { type: 'text', key: 'port', label: 'Port', description: 'HTTP server port', default: 8080, value_type: 'number', flag: '--port' },
    { type: 'dropdown', key: 'context_length', label: 'Context Length', description: 'Max tokens', default: 4096, options: [2048, 4096, 8192, 16384, 32768, 65536, 131072], flag: '-c' },
    { type: 'section', label: 'Inference' },
    { type: 'text', key: 'threads', label: 'Threads', description: 'CPU threads', default: 'auto', value_type: 'string', flag: '-t' },
    { type: 'text', key: 'batch_size', label: 'Batch Size', description: 'Prompt processing', default: 512, value_type: 'number', flag: '-b' },
    { type: 'section', label: 'Features' },
    { type: 'bool', key: 'enable_web_ui', label: 'Enable Web UI', description: 'Serve built-in chat interface', default: true, flag: null },
    { type: 'bool', key: 'flash_attention', label: 'Flash Attention', description: 'Faster inference', default: true, flag: '-fa' },
  ];
}

// ============================================================================
// Initialization
// ============================================================================

async function init() {
  try {
    state.versionGroups = await invoke('get_llamafile_versions');
    if (state.versionGroups.length > 0) {
      state.selectedVersionGroup = state.versionGroups[0].name;
      state.selectedRelease = state.versionGroups[0].releases[0];
    }
    await updateResolvedPath();

    state.models = await invoke('get_models');
    state.filteredModels = [...state.models];

    // Load model download directory
    try {
      state.modelDownloadDir = await invoke('get_model_cache_dir') || '';
    } catch (e) {
      state.modelDownloadDir = '~/.llamafile/models';
    }

    // Select first model using selectModel flow
    if (state.models.length > 0) {
      selectModel(state.models[0].id);
    }

    // Load config schema and initialize defaults
    state.configSchema = await invoke('get_config_schema');
    state.configValues = {};
    for (const item of state.configSchema) {
      if (item.type !== 'section' && item.key) {
        state.configValues[item.key] = item.default;
      }
    }

    state.agentTools = getMockAgentTools();

    if (state.selectedModel) {
      state.outputPath = await invoke('get_default_output_path', {
        model_name: state.selectedModel.name
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
    state.downloadProgress = event.payload;
    renderDownloadProgress();
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
      <li class="${classes}" data-step="${step.id}" onclick="goToStep(${step.id})">
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
          const isGroupSelected = state.llamafileSource === 'registry' && state.selectedVersionGroup === g.name;
          const isExpanded = !!state.expandedGroups[g.name];
          const hasMultiple = g.releases.length > 1;
          const shortVersion = g.name.replace('llamafile ', '');

          return `
            <div class="version-group ${isGroupSelected ? 'selected' : ''}">
              <div class="selection-card ${isGroupSelected ? 'selected' : ''}"
                   onclick="selectVersionGroup('${g.name}')">
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
                  <div class="expand-toggle" onclick="event.stopPropagation(); toggleVersionGroup('${g.name}')">
                    <span class="expand-icon ${isExpanded ? 'expanded' : ''}">&#9654;</span>
                    <span class="expand-count">${g.releases.length}</span>
                  </div>
                ` : ''}
              </div>
              ${hasMultiple && isExpanded ? `
                <div class="version-releases">
                  ${g.releases.map(r => {
                    const isSelected = state.selectedRelease?.version === r.version;
                    return `
                      <div class="release-row ${isSelected ? 'selected' : ''}"
                           onclick="selectSpecificRelease('${g.name}', '${r.version}')">
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
             onclick="${state.llamafileSource === 'custom' ? '' : 'selectCustomLlamafile()'}">
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
            <button class="custom-clear-btn" onclick="event.stopPropagation(); selectCustomLlamafile()">Change</button>
            <button class="custom-clear-btn" onclick="event.stopPropagation(); clearCustomLlamafile()">Clear</button>
          ` : `
            <div class="card-meta" style="color: var(--text-secondary);">Browse&hellip;</div>
          `}
        </div>
      </div>
      <div class="footer">
        <div></div>
        <button class="btn btn-primary" onclick="nextStep()">Next &rarr;</button>
      </div>
    </div>
  `;
}

async function updateResolvedPath() {
  if (state.llamafileSource === 'custom') {
    state.resolvedLlamafilePath = state.customLlamafilePath;
  } else if (state.selectedRelease) {
    try {
      const cacheDir = await invoke('get_llamafile_cache_dir', {
        version: state.selectedRelease.version,
      });
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

function selectVersionGroup(name) {
  const group = state.versionGroups.find(g => g.name === name);
  if (!group) return;
  state.selectedVersionGroup = name;
  state.selectedRelease = group.releases[0];
  state.llamafileSource = 'registry';
  state.customLlamafilePath = null;
  updateResolvedPath();
  renderCurrentStep();
}

function selectSpecificRelease(groupName, version) {
  const group = state.versionGroups.find(g => g.name === groupName);
  if (!group) return;
  const release = group.releases.find(r => r.version === version);
  if (!release) return;
  state.selectedVersionGroup = groupName;
  state.selectedRelease = release;
  state.llamafileSource = 'registry';
  state.customLlamafilePath = null;
  updateResolvedPath();
  renderCurrentStep();
}

async function selectCustomLlamafile() {
  try {
    const path = await invoke('select_llamafile_path');
    if (path) {
      state.llamafileSource = 'custom';
      state.customLlamafilePath = path;
      state.selectedVersionGroup = null;
      state.selectedRelease = null;
      updateResolvedPath();
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to select custom llamafile:', err);
  }
}

function clearCustomLlamafile() {
  state.llamafileSource = 'registry';
  state.customLlamafilePath = null;
  if (state.versionGroups.length > 0) {
    state.selectedVersionGroup = state.versionGroups[0].name;
    state.selectedRelease = state.versionGroups[0].releases[0];
  }
  updateResolvedPath();
  renderCurrentStep();
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
                 value="${state.searchQuery}" oninput="searchModels(this.value)">
        </div>

        <div class="filter-row">
          ${chips.map(c => `
            <div class="filter-chip ${state.activeFilter === c.value ? 'active' : ''}"
                 onclick="filterModels('${c.value}')">${c.label}</div>
          `).join('')}
        </div>

        <div class="download-dir-row">
          <span class="download-dir-path" title="${state.modelDownloadDir}">Download model files in: ${state.modelDownloadDir}</span>
          <button class="download-dir-btn" onclick="event.stopPropagation(); selectDownloadDir()">\ud83d\udcc2</button>
        </div>

        ${state.filteredModels.map(m => {
          const isSelected = !isCustomMode && state.selectedModel?.id === m.id;
          const weightsFile = m.files.find(f => f.role === 'weights');
          const firstQuant = weightsFile?.quantizations[0];
          const totalSize = getTotalModelSizeForModel(m);

          return `
            <div class="model-card-wrapper">
              <div class="selection-card ${isSelected ? 'selected' : ''}"
                   onclick="selectModel('${m.id}')">
                <div class="radio-dot"></div>
                <div class="card-info">
                  <div class="card-title">${m.huggingface_url
                    ? `<a href="${m.huggingface_url}" class="model-link" onclick="openExternalLink(event, '${m.huggingface_url}')">${m.name}</a>`
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
      </div>
      <div class="footer">
        <button class="btn btn-secondary" onclick="prevStep()">\u2190 Back</button>
        <button class="btn btn-primary" onclick="nextStep()" ${!isModelStepValid() ? 'disabled' : ''}>Next \u2192</button>
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
                  <div class="expand-toggle" onclick="event.stopPropagation(); toggleModelFileExpand('${f.role}')">
                    <span class="expand-icon ${isExpanded ? 'expanded' : ''}">\u25b6</span>
                    <span class="expand-count">${f.quantizations.length}</span>
                  </div>
                ` : ''}
                <button class="file-or-disk" onclick="event.stopPropagation(); selectCustomFileForRole('${f.role}')">or load from disk</button>
              ` : `
                <span class="custom-path-display" title="${sel.customPath || ''}">${getFileName(sel.customPath)}</span>
                <button class="custom-clear-btn" onclick="event.stopPropagation(); clearCustomFileForRole('${f.role}')">Clear</button>
              `}
            </div>
            ${!isCustomFile && hasMultipleQuants && isExpanded ? `
              <div class="version-releases">
                ${f.quantizations.map((q, qi) => {
                  const isQSelected = sel.quantizationIdx === qi;
                  return `
                    <div class="release-row ${isQSelected ? 'selected' : ''}"
                         onclick="selectFileQuantization('${f.role}', ${qi})">
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
      <div class="custom-llamafile-card active" onclick="">
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
          <button class="custom-add-btn" onclick="event.stopPropagation(); addCompanionFile()">+ Add file</button>
        </div>
        <button class="custom-clear-btn" onclick="event.stopPropagation(); clearCustomModel()">Clear</button>
      </div>
    `;
  }

  return `
    <div class="custom-llamafile-card ${isActive ? 'active' : ''}"
         onclick="selectCustomModel()">
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

function selectModel(modelId) {
  const model = state.models.find(m => m.id === modelId);
  if (!model) return;
  state.selectedModel = model;
  state.modelSource = 'registry';
  state.customModelFiles = [];
  state.configValues.context_length = model.default_context || 4096;
  initModelFileSelections(model);
  renderCurrentStep();
  invoke('get_default_output_path', { model_name: model.name })
    .then(path => { state.outputPath = path; });
}

function toggleModelFileExpand(role) {
  state.expandedModelFiles[role] = !state.expandedModelFiles[role];
  renderCurrentStep();
}

function selectFileQuantization(role, idx) {
  if (state.modelFileSelections[role]) {
    state.modelFileSelections[role].quantizationIdx = idx;
    state.modelFileSelections[role].source = 'registry';
    state.modelFileSelections[role].customPath = null;
  }
  renderCurrentStep();
}

async function selectCustomFileForRole(role) {
  try {
    const path = await invoke('select_model_file_path');
    if (path && state.modelFileSelections[role]) {
      state.modelFileSelections[role].source = 'custom';
      state.modelFileSelections[role].customPath = path;
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to select custom file for role:', err);
  }
}

function clearCustomFileForRole(role) {
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
      state.modelSource = 'custom';
      state.selectedModel = null;
      state.modelFileSelections = {};
      state.expandedModelFiles = {};
      state.customModelFiles = [{ role: 'weights', path }];
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
      state.customModelFiles.push({ role, path });
      renderCurrentStep();
    }
  } catch (err) {
    console.error('Failed to add companion file:', err);
  }
}

function clearCustomModel() {
  state.modelSource = 'registry';
  state.customModelFiles = [];
  // Re-select first model if available
  if (state.models.length > 0) {
    selectModel(state.models[0].id);
  } else {
    state.selectedModel = null;
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
        <button class="btn btn-secondary" onclick="prevStep()">\u2190 Back</button>
        <button class="btn btn-primary" onclick="nextStep()">Next \u2192</button>
      </div>
    </div>
  `;
}

function renderConfigItems() {
  let html = '';
  let sectionOpen = false;

  for (const item of state.configSchema) {
    if (item.type === 'section') {
      if (sectionOpen) html += '</div>';
      html += `<div class="config-section"><div class="config-section-title">${item.label}</div>`;
      sectionOpen = true;
    } else if (item.type === 'text') {
      html += renderTextItem(item);
    } else if (item.type === 'dropdown') {
      html += renderDropdownItem(item);
    } else if (item.type === 'bool') {
      html += renderBoolItem(item);
    }
  }
  if (sectionOpen) html += '</div>';
  return html;
}

function renderTextItem(item) {
  const val = state.configValues[item.key];
  const inputType = item.value_type === 'number' ? 'number' : 'text';
  const handler = item.value_type === 'number'
    ? `updateConfig('${item.key}', parseInt(this.value))`
    : `updateConfig('${item.key}', this.value)`;
  return `
    <div class="config-row">
      <div class="config-label">${item.label}<div class="config-label-sub">${item.description}</div></div>
      <input class="config-input" type="${inputType}" value="${val}" onchange="${handler}">
    </div>
  `;
}

function renderDropdownItem(item) {
  const val = state.configValues[item.key];
  const isNumeric = item.options.length > 0 && typeof item.options[0] === 'number';
  const handler = isNumeric
    ? `updateConfig('${item.key}', parseInt(this.value))`
    : `updateConfig('${item.key}', this.value)`;
  return `
    <div class="config-row">
      <div class="config-label">${item.label}<div class="config-label-sub">${item.description}</div></div>
      <select class="config-select" onchange="${handler}">
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
    <div class="toggle-row" onclick="toggleConfig('${item.key}')">
      <div class="toggle-info">
        <div class="toggle-title">${item.label}</div>
        <div class="toggle-desc">${item.description}</div>
      </div>
      <div class="toggle-switch ${val ? 'on' : ''}"></div>
    </div>
  `;
}

function updateConfig(key, value) { state.configValues[key] = value; }
function toggleConfig(key) { state.configValues[key] = !state.configValues[key]; renderCurrentStep(); }

// ============================================================================
// Step 4: Compute
// ============================================================================

function renderComputeStep() {
  const modelSize = getTotalModelSize();
  const estimatedSize = estimateTotalSize();
  const sizePercent = Math.min((estimatedSize / 10e9) * 100, 100);

  return `
    <div class="step-panel active">
      <div class="content-header">
        <div class="content-title">Compute Target</div>
        <div class="content-subtitle">Choose whether to bundle GPU acceleration libraries.</div>
      </div>
      <div class="content-body">
        <div class="compute-grid">
          <div class="compute-card ${state.computeMode === 'cpu' ? 'selected' : ''}" onclick="selectComputeMode('cpu')">
            <div class="compute-card-icon">\ud83e\uddf3</div>
            <div class="compute-card-title">CPU Only</div>
            <div class="compute-card-desc">Lean build, runs on any x86_64 or ARM64 machine.</div>
            <div class="compute-card-meta">
              <span class="meta-chip">x86_64</span>
              <span class="meta-chip">ARM64</span>
              <span class="meta-chip">~5 MB</span>
            </div>
          </div>
          <div class="compute-card ${state.computeMode === 'gpu' ? 'selected' : ''}" onclick="selectComputeMode('gpu')">
            <div class="compute-card-icon">\ud83d\udd25</div>
            <div class="compute-card-title">GPU Accelerated</div>
            <div class="compute-card-desc">Bundles CUDA, ROCm, and Metal support.</div>
            <div class="compute-card-meta">
              <span class="meta-chip">CUDA</span>
              <span class="meta-chip">ROCm</span>
              <span class="meta-chip">Metal</span>
            </div>
          </div>
        </div>

        <div class="config-section" style="opacity: ${state.computeMode === 'gpu' ? 1 : 0.3}; pointer-events: ${state.computeMode === 'gpu' ? 'all' : 'none'};">
          <div class="config-section-title">GPU Options</div>
          <div class="toggle-row" onclick="toggleGpuOption('cuda')">
            <div class="toggle-info"><div class="toggle-title">NVIDIA CUDA</div><div class="toggle-desc">~180 MB</div></div>
            <div class="toggle-switch ${state.gpuOptions.cuda ? 'on' : ''}"></div>
          </div>
          <div class="toggle-row" onclick="toggleGpuOption('rocm')">
            <div class="toggle-info"><div class="toggle-title">AMD ROCm</div><div class="toggle-desc">~60 MB</div></div>
            <div class="toggle-switch ${state.gpuOptions.rocm ? 'on' : ''}"></div>
          </div>
          <div class="toggle-row" onclick="toggleGpuOption('metal')">
            <div class="toggle-info"><div class="toggle-title">Apple Metal</div><div class="toggle-desc">~10 MB</div></div>
            <div class="toggle-switch ${state.gpuOptions.metal ? 'on' : ''}"></div>
          </div>
        </div>

        <div class="size-estimate">
          <div class="size-label">Estimated total size</div>
          <div class="size-bar"><div class="size-fill" style="width: ${sizePercent}%"></div></div>
          <div class="size-text">${formatBytes(estimatedSize)}</div>
        </div>
      </div>
      <div class="footer">
        <button class="btn btn-secondary" onclick="prevStep()">\u2190 Back</button>
        <button class="btn btn-primary" onclick="nextStep()">Next \u2192</button>
      </div>
    </div>
  `;
}

function selectComputeMode(mode) { state.computeMode = mode; renderCurrentStep(); }
function toggleGpuOption(opt) { state.gpuOptions[opt] = !state.gpuOptions[opt]; renderCurrentStep(); }

function estimateTotalSize() {
  const modelSize = getTotalModelSize();
  let overhead = 5e6;
  if (state.computeMode === 'gpu') {
    if (state.gpuOptions.cuda) overhead += 180e6;
    if (state.gpuOptions.rocm) overhead += 60e6;
    if (state.gpuOptions.metal) overhead += 10e6;
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
        <div class="agent-toggle-big ${state.agentEnabled ? 'active' : ''}" onclick="toggleAgent()">
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
              <div class="tool-card ${t.enabled ? 'selected' : ''}" onclick="toggleTool('${t.id}')">
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
              <textarea onchange="updateSystemPrompt(this.value)">${state.systemPrompt}</textarea>
            </div>
          </div>
        </div>
      </div>
      <div class="footer">
        <button class="btn btn-secondary" onclick="prevStep()">\u2190 Back</button>
        <button class="btn btn-primary" onclick="nextStep()">Next \u2192</button>
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
            <div class="summary-edit" onclick="goToStep(1)">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Model</div>
            <div class="summary-value">${renderModelSummaryValue()}</div>
            <div class="summary-edit" onclick="goToStep(2)">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Context</div>
            <div class="summary-value">${(state.configValues.context_length || 4096).toLocaleString()} tokens</div>
            <div class="summary-edit" onclick="goToStep(3)">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Compute</div>
            <div class="summary-value">${state.computeMode === 'gpu' ? 'GPU (' + getGpuSummary() + ')' : 'CPU Only'}</div>
            <div class="summary-edit" onclick="goToStep(4)">edit</div>
          </div>
          <div class="summary-row">
            <div class="summary-label">Agent</div>
            <div class="summary-value">${state.agentEnabled ? `Enabled \u2014 ${enabledTools.length} tools` : 'Disabled'}</div>
            <div class="summary-edit" onclick="goToStep(5)">edit</div>
          </div>
        </div>

        <div class="output-path" onclick="selectOutputPath()">
          <div class="output-path-label">Output:</div>
          <div class="output-path-value">${state.outputPath || 'Select location...'}</div>
          <div class="output-path-btn">Browse\u2026</div>
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
              <input type="password" class="hf-token-input" id="hf-token-input" placeholder="hf_..." onkeydown="if(event.key==='Enter') submitHfToken(this.value)">
              <div class="hf-token-buttons">
                <button class="btn btn-secondary" onclick="cancelHfTokenPrompt()">Cancel</button>
                <button class="btn btn-primary" onclick="submitHfToken(document.getElementById('hf-token-input').value)">Save &amp; Retry</button>
              </div>
            </div>
          </div>
        ` : ''}

        ${state.fileExistsPrompt ? `
          <div class="hf-token-modal">
            <div class="hf-token-content">
              <div class="hf-token-title">File Already Exists</div>
              <div class="hf-token-desc">
                <code>${state.fileExistsPrompt.filename}</code> already exists in the cache.<br>
                Would you like to use the existing file or re-download it?
              </div>
              <div class="hf-token-buttons" style="margin-top: 16px;">
                <button class="btn btn-secondary" onclick="handleFileExistsChoice('skip')">Use Existing</button>
                <button class="btn btn-primary" onclick="handleFileExistsChoice('download')">Re-download</button>
              </div>
            </div>
          </div>
        ` : ''}
      </div>
      <div class="footer">
        <button class="btn btn-secondary" onclick="prevStep()" ${state.isBuilding ? 'disabled' : ''}>\u2190 Back</button>
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
    return `<button class="btn btn-success" onclick="openOutputFolder()"><img src="llamafile-icon.png" style="width:16px;height:16px;"> Show in Finder</button>`;
  }
  if (state.isBuilding) {
    return `<button class="btn btn-secondary" onclick="cancelBuild()">Cancel</button>`;
  }
  return `<button class="btn btn-primary" onclick="startBuild()">\u26a1 Build llamafile</button>`;
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
  const el = document.getElementById('download-progress-text');
  if (!el || !state.downloadProgress) return;

  const { filename, bytes_downloaded, bytes_total, percentage } = state.downloadProgress;
  const downloaded = formatBytes(bytes_downloaded);
  const total = bytes_total > 0 ? formatBytes(bytes_total) : '?';
  const pct = percentage.toFixed(1);

  el.textContent = `${filename}: ${downloaded} / ${total} (${pct}%)`;
}

// Download a file, handling auth errors by prompting for HF token
async function downloadWithAuth(url, destPath, sha256) {
  try {
    return await invoke('download_file', {
      url,
      destPath,
      expectedSha256: sha256 || null,
    });
  } catch (err) {
    if (err === 'AUTH_REQUIRED') {
      // Store pending retry info and show token prompt
      state.hfTokenPendingRetry = { url, destPath, sha256 };
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
      const { url, destPath, sha256 } = state.hfTokenPendingRetry;
      state.hfTokenPendingRetry = null;
      renderCurrentStep();

      try {
        const result = await invoke('download_file', {
          url,
          destPath,
          expectedSha256: sha256 || null,
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
// Returns: 'skip' (use existing), 'download' (re-download), or throws on cancel
async function checkFileExistsWithPrompt(path, sha256) {
  const exists = await invoke('file_exists', { path, expectedSha256: sha256 });
  if (!exists) {
    return 'download';
  }

  // File exists - prompt user
  const filename = getFilenameFromUrl(path) || path.split('/').pop();

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

async function selectOutputPath() {
  try {
    const path = await invoke('select_output_path', { default_name: `${state.selectedModel?.name || 'model'}.llamafile` });
    if (path) { state.outputPath = path; renderCurrentStep(); }
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
      const cacheDir = await invoke('get_llamafile_cache_dir', { version: release.version });
      const filename = getFilenameFromUrl(release.download_url);
      const cachePath = `${cacheDir}/${filename}`;

      const action = await checkFileExistsWithPrompt(cachePath, release.sha256);

      if (action === 'download' && release.download_url) {
        state.buildLog.push(`Downloading llamafile ${release.version}...`);
        renderBuildLog();
        try {
          llamafilePath = await downloadWithAuth(release.download_url, cachePath, release.sha256);
          state.buildLog.push(`Downloaded: ${cachePath}`);
          renderBuildLog();
        } catch (err) {
          state.buildLog.push(`Download failed: ${err}`);
          renderBuildLog();
          throw err;
        }
      } else {
        llamafilePath = cachePath;
        state.buildLog.push(`Using existing: ${cachePath}`);
        renderBuildLog();
      }
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
          // User-provided file
          modelFiles.push({ role: file.role, path: sel.customPath });
          state.buildLog.push(`Using custom ${file.role}: ${sel.customPath}`);
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

          const action = await checkFileExistsWithPrompt(destPath, quant.sha256);

          if (action === 'download') {
            state.buildLog.push(`Downloading ${file.label || file.role} (${quant.name})...`);
            renderBuildLog();
            try {
              const path = await downloadWithAuth(quant.download_url, destPath, quant.sha256);
              modelFiles.push({ role: file.role, path });
              state.buildLog.push(`Downloaded: ${destPath}`);
              renderBuildLog();
            } catch (err) {
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
      // Custom model files
      for (const f of state.customModelFiles) {
        modelFiles.push({ role: f.role, path: f.path });
      }
    }

    state.buildProgress = { stage: 'downloading_model', progress_pct: 60 };
    renderCurrentStep();

    // Step 3: Write config
    state.buildProgress = { stage: 'writing_config', progress_pct: 70, message: 'Writing configuration...' };
    renderCurrentStep();
    state.buildLog.push('Writing .args configuration...');
    renderBuildLog();
    await new Promise(r => setTimeout(r, 300)); // Placeholder for actual config writing

    // Step 4: Bundle GPU (placeholder)
    state.buildProgress = { stage: 'bundling_gpu', progress_pct: 85, message: 'Bundling GPU libraries...' };
    renderCurrentStep();
    state.buildLog.push(`Compute mode: ${state.computeMode}`);
    if (state.computeMode === 'gpu') {
      const gpus = [];
      if (state.gpuOptions.cuda) gpus.push('CUDA');
      if (state.gpuOptions.rocm) gpus.push('ROCm');
      if (state.gpuOptions.metal) gpus.push('Metal');
      state.buildLog.push(`GPU targets: ${gpus.join(', ') || 'none'}`);
    }
    renderBuildLog();
    await new Promise(r => setTimeout(r, 300));

    // Step 5: Create executable
    state.buildProgress = { stage: 'creating_executable', progress_pct: 95, message: 'Creating executable...' };
    renderCurrentStep();
    state.buildLog.push(`Creating executable at: ${state.outputPath}`);
    state.buildLog.push(`Llamafile: ${llamafilePath}`);
    for (const f of modelFiles) {
      state.buildLog.push(`Model ${f.role}: ${f.path}`);
    }
    renderBuildLog();

    // Run zipalign
    try {
      await invoke('run_zipalign', { args: [] });
    } catch (err) {
      state.buildLog.push(`zipalign error: ${err}`);
      renderBuildLog();
    }

    // Done
    state.buildProgress = { stage: 'complete', progress_pct: 100, message: 'Build complete!' };
    state.buildLog.push('Build complete!');
    renderBuildLog();

  } catch (err) {
    state.buildProgress = { stage: 'error', progress_pct: 0, message: `Error: ${err}` };
    state.buildLog.push(`Build failed: ${err}`);
    renderBuildLog();
  }

  state.isBuilding = false;
  state.downloadProgress = null;
  renderCurrentStep();
}

async function cancelBuild() { state.isBuilding = false; state.buildProgress = null; renderCurrentStep(); }
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

function goToStep(step) {
  if (step > state.maxVisitedStep) return;
  state.currentStep = step;
  renderSidebar();
  renderCurrentStep();
}

function nextStep() {
  if (state.currentStep < STEPS.length) {
    state.currentStep++;
    state.maxVisitedStep = Math.max(state.maxVisitedStep, state.currentStep);
    renderSidebar();
    renderCurrentStep();
  }
}

function prevStep() {
  if (state.currentStep > 1) {
    state.currentStep--;
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

function getFilenameFromUrl(url) {
  if (!url) return 'file';
  try {
    const parsed = new URL(url);
    return parsed.pathname.split('/').pop() || 'file';
  } catch {
    return url.split('/').pop() || 'file';
  }
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

// Global exports
window.goToStep = goToStep;
window.nextStep = nextStep;
window.prevStep = prevStep;
window.selectVersionGroup = selectVersionGroup;
window.selectSpecificRelease = selectSpecificRelease;
window.toggleVersionGroup = toggleVersionGroup;
window.selectModel = selectModel;
window.searchModels = searchModels;
window.filterModels = filterModels;
window.updateConfig = updateConfig;
window.toggleConfig = toggleConfig;
window.selectComputeMode = selectComputeMode;
window.toggleGpuOption = toggleGpuOption;
window.toggleAgent = toggleAgent;
window.toggleTool = toggleTool;
window.updateSystemPrompt = updateSystemPrompt;
window.selectCustomLlamafile = selectCustomLlamafile;
window.clearCustomLlamafile = clearCustomLlamafile;
window.selectOutputPath = selectOutputPath;
window.startBuild = startBuild;
window.cancelBuild = cancelBuild;
window.openOutputFolder = openOutputFolder;
window.toggleModelFileExpand = toggleModelFileExpand;
window.selectFileQuantization = selectFileQuantization;
window.selectCustomFileForRole = selectCustomFileForRole;
window.clearCustomFileForRole = clearCustomFileForRole;
window.selectCustomModel = selectCustomModel;
window.addCompanionFile = addCompanionFile;
window.clearCustomModel = clearCustomModel;
window.selectDownloadDir = selectDownloadDir;
window.submitHfToken = submitHfToken;
window.cancelHfTokenPrompt = cancelHfTokenPrompt;
window.handleFileExistsChoice = handleFileExistsChoice;
window.openExternalLink = openExternalLink;

document.addEventListener('DOMContentLoaded', init);
