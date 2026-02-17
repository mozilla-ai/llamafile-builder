// llamafile Builder - State Management
// Centralized state for the application

export const state = {
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
  modelSource: 'registry',       // 'registry', 'custom', 'ollama', or 'lmstudio'
  modelFileSelections: {},        // keyed by file role: { source, quantizationIdx, customPath }
  expandedModelFiles: {},         // keyed by file role: bool
  modelDownloadDir: '',
  customModelFiles: [],           // array of { role, path }
  searchQuery: '',
  activeFilter: 'all',

  // Ollama model support
  ollamaAvailable: false,         // Whether ~/.ollama exists with models
  ollamaModels: [],               // Array of OllamaModel objects from backend
  selectedOllamaModel: null,      // Currently selected Ollama model
  ollamaCacheDir: '',             // Custom Ollama cache directory (default: ~/.ollama)

  // LM Studio model support
  lmstudioAvailable: false,       // Whether LM Studio cache exists with models
  lmstudioModels: [],             // Array of LMStudioModel objects from backend
  selectedLMStudioModel: null,    // Currently selected LM Studio model
  lmstudioFileSelections: {},     // File selections keyed by role: { weights: idx, mmproj: idx }
  lmstudioCacheDir: '',           // Custom LM Studio cache directory
  expandedLMStudioFiles: {},      // UI state for expanded file lists

  // Resolved model file paths (computed by backend)
  resolvedWeightsPath: null,      // Full path to selected weights file
  resolvedWeightsFilename: null,  // Just the filename
  resolvedMmprojPath: null,       // Full path to selected mmproj file (if any)
  resolvedMmprojFilename: null,   // Just the filename

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

export const STEPS = [
  { id: 1, name: 'Version', icon: '1' },
  { id: 2, name: 'Model', icon: '2' },
  { id: 3, name: 'Configuration', icon: '3' },
  { id: 4, name: 'Compute', icon: '4' },
  { id: 5, name: 'Agent', icon: '5' },
  { id: 6, name: 'Build', icon: '6' },
];
