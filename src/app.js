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
      const baseName = args?.ggufFilename?.replace(/\.gguf$/i, '') || args?.modelName || 'model';
      return `/mock/Desktop/${baseName}.llamafile`;
    case 'get_system_info':
      return { os: 'macos', arch: 'aarch64', cpu_cores: 8, memory_gb: 16, has_metal: true };
    case 'select_llamafile_path':
      return '/mock/builds/llamafile';
    case 'get_llamafile_cache_dir':
      return '/mock/.llamafile/bin';
    case 'get_model_cache_dir':
      return '/mock/.llamafile/models';
    case 'select_model_file_path':
      return '/mock/models/custom-model.gguf';
    case 'select_model_download_dir':
      return '/mock/models';
    case 'select_output_path':
      return args?.defaultPath || '/mock/Desktop/output.llamafile';
    case 'generate_args_content':
      // Mock: return sample .args content
      return `-m\n/zip/${args?.modelFilename || 'model.gguf'}\n--no-mmap\n--host\n127.0.0.1\n--port\n8080\n--ctx-size\n4096\n--webui\n...`;
    case 'copy_file':
      console.log(`[Mock] copy_file: ${args?.src} -> ${args?.dest}`);
      return null;
    case 'write_temp_file':
      console.log(`[Mock] write_temp_file: ${args?.filename}`);
      return `/tmp/llamafile-builder/${args?.filename}`;
    case 'make_executable':
      console.log(`[Mock] make_executable: ${args?.path}`);
      return null;
    case 'run_zipalign':
      console.log(`[Mock] run_zipalign: ${args?.args?.join(' ')}`);
      return 0; // Success
    case 'download_file':
      // Mock: simulate download success
      console.log(`[Mock] download_file: ${args?.url} -> ${args?.destPath}`);
      return args?.destPath;
    case 'cancel_download':
      console.log('[Mock] cancel_download');
      return null;
    case 'get_file_info':
      // Mock: file doesn't exist
      return { exists: false, size: 0, expectedSize: args?.expectedSize, isComplete: false, isPartial: false, checksumValid: null };
    case 'file_exists':
      // Mock: files don't exist (trigger download)
      return false;
    case 'delete_file':
      console.log(`[Mock] delete_file: ${args?.path}`);
      return null;
    case 'calculate_sha256':
      console.log(`[Mock] calculate_sha256: ${args?.path}`);
      return 'mock_sha256_' + Math.random().toString(36).substring(7);
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
    { type: 'bool', key: 'enable_web_ui', label: 'Enable Web UI', description: 'Serve built-in chat interface', default: true, flag: '--webui' },
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

    state.agentTools = getMockAgentTools();

    if (state.selectedModel) {
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
                <button class="btn btn-primary" onclick="handleShaVerifyChoice('ok')">OK</button>
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
                <button class="btn btn-primary" onclick="handleShaVerifyChoice('verify')">Verify SHA256</button>
                <button class="btn btn-secondary" onclick="handleShaVerifyChoice('skip')">Skip (use as custom)</button>
                <button class="btn btn-secondary" onclick="handleShaVerifyChoice('cancel')" style="margin-left: auto;">Cancel</button>
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
                <button class="btn btn-primary" onclick="handleShaVerifyChoice('use_custom')">OK, Use as Custom</button>
              </div>
            </div>
          </div>
        ` : ''}
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
  invoke('get_default_output_path', {
    ggufFilename: getSelectedGgufFilename(),
    modelName: model.name
  }).then(path => { state.outputPath = path; });
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
  // Update output filename if weights changed
  if (role === 'weights') {
    updateDefaultOutputPath();
  }
}

async function updateDefaultOutputPath() {
  if (state.selectedModel || state.modelSource === 'custom') {
    state.outputPath = await invoke('get_default_output_path', {
      ggufFilename: getSelectedGgufFilename(),
      modelName: state.selectedModel?.name
    });
    renderCurrentStep();
  }
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

function switchToCustomModelWithFile(role, path) {
  // Switch to custom model mode with this file
  state.modelSource = 'custom';
  state.customModelFiles = [{ role, path }];
  state.selectedModel = null;
  renderCurrentStep();
}

function handleShaVerifyChoice(choice) {
  if (state.shaVerifyPrompt?.resolve) {
    state.shaVerifyPrompt.resolve(choice);
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
            <div class="config-section-header" onclick="toggleConfig('${item.key}')">
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
    }
  }

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

        <div class="args-preview">
          <div class="args-preview-header">
            <div class="args-preview-label">.args Configuration</div>
            <button class="args-preview-refresh" onclick="refreshArgsContent().then(() => renderCurrentStep())" title="Regenerate from settings">Refresh</button>
          </div>
          <textarea
            class="args-preview-editor"
            spellcheck="false"
            onchange="updateArgsContent(this.value)"
            oninput="updateArgsContent(this.value)"
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
        ` : ''}

        ${state.cancelConfirmVisible ? `
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
    // Use current outputPath as default (includes directory), or build one
    const defaultPath = state.outputPath ||
      await invoke('get_default_output_path', {
        ggufFilename: getSelectedGgufFilename(),
        modelName: state.selectedModel?.name
      });
    const path = await invoke('select_output_path', { defaultPath });
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

    // Generate .args content
    const argsContent = await invoke('generate_args_content', {
      configValues: state.configValues,
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

    // Step 4: Bundle GPU (placeholder for future)
    state.buildProgress = { stage: 'bundling_gpu', progress_pct: 80, message: 'Bundling GPU libraries...' };
    renderCurrentStep();
    state.buildLog.push(`Compute mode: ${state.computeMode}`);
    if (state.computeMode === 'gpu') {
      const gpus = [];
      if (state.gpuOptions.cuda) gpus.push('CUDA');
      if (state.gpuOptions.rocm) gpus.push('ROCm');
      if (state.gpuOptions.metal) gpus.push('Metal');
      state.buildLog.push(`GPU targets: ${gpus.join(', ') || 'none'} (not yet implemented)`);
    }
    renderBuildLog();

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
    if (state.currentStep === 3) {
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
    // Skip steps 4, 5 - go back from Build to Configuration
    if (state.currentStep === 6) {
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
window.refreshArgsContent = refreshArgsContent;
window.updateArgsContent = updateArgsContent;
window.startBuild = startBuild;
window.cancelBuild = cancelBuild;
window.confirmCancelBuild = confirmCancelBuild;
window.dismissCancelConfirm = dismissCancelConfirm;
window.openOutputFolder = openOutputFolder;
window.toggleModelFileExpand = toggleModelFileExpand;
window.selectFileQuantization = selectFileQuantization;
window.selectCustomFileForRole = selectCustomFileForRole;
window.clearCustomFileForRole = clearCustomFileForRole;
window.handleShaVerifyChoice = handleShaVerifyChoice;
window.selectCustomModel = selectCustomModel;
window.addCompanionFile = addCompanionFile;
window.clearCustomModel = clearCustomModel;
window.selectDownloadDir = selectDownloadDir;
window.submitHfToken = submitHfToken;
window.cancelHfTokenPrompt = cancelHfTokenPrompt;
window.handleFileExistsChoice = handleFileExistsChoice;
window.openExternalLink = openExternalLink;

document.addEventListener('DOMContentLoaded', init);
