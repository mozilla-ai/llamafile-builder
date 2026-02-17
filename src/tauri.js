// llamafile Builder - Tauri Bindings
// Direct Tauri invoke/listen with optional mock fallback for browser development

// Check if we're running in Tauri or browser
const isTauri = typeof window !== 'undefined' && window.__TAURI__;

// Mock data cache for browser development
const mockDataCache = {
  versions: null,
  models: null,
  configSchema: null,
};

async function getMockVersions() {
  if (mockDataCache.versions) return mockDataCache.versions;
  try {
    const response = await fetch('../src-tauri/versions.json');
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    mockDataCache.versions = await response.json();
    console.log('[Mock] Loaded versions.json:', mockDataCache.versions.length, 'version groups');
    return mockDataCache.versions;
  } catch (e) {
    console.error('[Mock] Failed to load versions.json:', e);
    return [];
  }
}

async function getMockModels() {
  if (mockDataCache.models) return mockDataCache.models;
  try {
    const response = await fetch('../src-tauri/models.json');
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    mockDataCache.models = await response.json();
    console.log('[Mock] Loaded models.json:', mockDataCache.models.length, 'models');
    return mockDataCache.models;
  } catch (e) {
    console.error('[Mock] Failed to load models.json:', e);
    return [];
  }
}

async function getMockConfigSchema() {
  if (mockDataCache.configSchema) return mockDataCache.configSchema;
  try {
    const response = await fetch('../src-tauri/config.json');
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    mockDataCache.configSchema = await response.json();
    console.log('[Mock] Loaded config.json:', mockDataCache.configSchema.length, 'items');
    return mockDataCache.configSchema;
  } catch (e) {
    console.error('[Mock] Failed to load config.json:', e);
    return [];
  }
}

function getMockBuildState() {
  return {
    modelSource: 'registry',
    selectedModelId: null,
    modelFileSelections: {},
    customModelFiles: [],
    selectedOllamaModel: null,
    selectedLmstudioModel: null,
    lmstudioFileSelections: {},
    llamafileSource: 'registry',
    selectedVersionGroup: null,
    selectedReleaseVersion: null,
    customLlamafilePath: null,
    configValues: { context_length: 4096 },
    outputPath: '/mock/Desktop/output.llamafile',
    resolvedLlamafilePath: null,
    resolvedWeightsPath: null,
    resolvedWeightsFilename: null,
    resolvedMmprojPath: null,
    resolvedMmprojFilename: null,
  };
}

// Mock invoke implementation for browser development
async function mockInvoke(cmd, args) {
  console.log(`[Mock] invoke(${cmd})`, args);
  switch (cmd) {
    case 'get_llamafile_versions':
      return await getMockVersions();
    case 'get_models':
      return await getMockModels();
    case 'get_config_schema':
      return await getMockConfigSchema();
    case 'search_models':
      const models = await getMockModels();
      return models.filter(m =>
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
    case 'get_gpu_lib_cache_dir':
      return `/mock/.llamafile/v/${args?.version || 'unknown'}`;
    case 'select_model_file_path':
      return '/mock/models/custom-model.gguf';
    case 'select_model_download_dir':
      return '/mock/models';
    case 'select_output_path':
      return args?.defaultPath || '/mock/Desktop/output.llamafile';
    case 'generate_args_content':
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
      return 0;
    case 'download_file':
      console.log(`[Mock] download_file: ${args?.url} -> ${args?.destPath}`);
      return args?.destPath;
    case 'cancel_download':
      console.log('[Mock] cancel_download');
      return null;
    case 'get_file_info':
      return { exists: false, size: 0, expectedSize: args?.expectedSize, isComplete: false, isPartial: false, checksumValid: null };
    case 'file_exists':
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
    // Build state commands
    case 'get_build_state':
      return getMockBuildState();
    case 'select_registry_model':
      return { ...getMockBuildState(), modelSource: 'registry', selectedModelId: args?.modelId };
    case 'select_custom_model':
      return { ...getMockBuildState(), modelSource: 'custom', selectedModelId: null, customModelFiles: [{ role: 'weights', path: args?.path }], outputPath: `/mock/Desktop/${args?.path?.split('/').pop()?.replace('.gguf', '')}.llamafile` };
    case 'add_custom_companion_file':
      return getMockBuildState();
    case 'remove_custom_companion_file':
      return getMockBuildState();
    case 'select_file_quantization':
      return getMockBuildState();
    case 'use_custom_file_for_role':
      return { ...getMockBuildState(), outputPath: `/mock/Desktop/${args?.path?.split('/').pop()?.replace('.gguf', '')}.llamafile` };
    case 'clear_custom_file_for_role':
      return getMockBuildState();
    case 'select_llamafile_version':
      return { ...getMockBuildState(), llamafileSource: 'registry', selectedVersionGroup: args?.groupName, selectedReleaseVersion: args?.version };
    case 'use_custom_llamafile':
      return { ...getMockBuildState(), llamafileSource: 'custom', customLlamafilePath: args?.path };
    case 'set_config_value':
      return getMockBuildState();
    case 'set_output_path':
      return { ...getMockBuildState(), outputPath: args?.path };
    // Ollama commands
    case 'get_default_ollama_cache_dir':
      return '~/.ollama';
    case 'check_ollama_available':
      return false;
    case 'get_ollama_models':
      return [];
    case 'select_ollama_model':
      return { ...getMockBuildState(), modelSource: 'ollama', selectedOllamaModel: { modelName: args?.modelName, tag: args?.tag, displayName: `${args?.modelName}:${args?.tag}`, weightsPath: '/mock/ollama/blob', sizeBytes: 0 }, outputPath: `/mock/Desktop/${args?.modelName}_${args?.tag}.llamafile` };
    // LM Studio commands
    case 'get_default_lmstudio_cache_dir':
      return '~/.cache/lm-studio';
    case 'check_lmstudio_available':
      return false;
    case 'get_lmstudio_models':
      return [];
    case 'select_lmstudio_model':
      return { ...getMockBuildState(), modelSource: 'lmstudio', selectedLmstudioModel: { dirName: args?.dirName, displayName: args?.dirName, files: [] }, lmstudioFileSelections: {}, outputPath: `/mock/Desktop/${args?.dirName}.llamafile` };
    case 'select_lmstudio_file':
      return getMockBuildState();
    default:
      return null;
  }
}

// Mock listen implementation for browser development
function mockListen(event, callback) {
  console.log(`[Mock] listen(${event})`);
  return () => {};
}

// Export the appropriate bindings based on environment
export const invoke = isTauri
  ? window.__TAURI__.core.invoke
  : mockInvoke;

export const listen = isTauri
  ? window.__TAURI__.event.listen
  : mockListen;
