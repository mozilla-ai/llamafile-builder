// llamafile Builder - LM Studio Model Provider
// Handles loading and selecting models from local LM Studio cache

import { state } from './state.js';
import { invoke } from './tauri.js';

// ============================================================================
// Provider Configuration
// ============================================================================

export const PROVIDER_ID = 'lmstudio';
export const PROVIDER_NAME = 'LM Studio';
export const PROVIDER_ICON = '📦';
export const PROVIDER_COLOR = 'blue';
export const DEFAULT_CACHE_DIR = '~/.cache/lm-studio';

// ============================================================================
// State Keys (added to main state)
// ============================================================================

export const stateKeys = {
  available: 'lmstudioAvailable',
  models: 'lmstudioModels',
  selectedModel: 'selectedLMStudioModel',
  fileSelections: 'lmstudioFileSelections',
  cacheDir: 'lmstudioCacheDir',
  expandedFiles: 'expandedLMStudioFiles',
};

// ============================================================================
// Default State Values
// ============================================================================

export const defaultState = {
  lmstudioAvailable: false,
  lmstudioModels: [],
  selectedLMStudioModel: null,
  lmstudioFileSelections: {},
  lmstudioCacheDir: '',
  expandedLMStudioFiles: {},
};

// ============================================================================
// Initialization
// ============================================================================

export async function init() {
  try {
    state.lmstudioCacheDir = await invoke('get_default_lmstudio_cache_dir');
    state.lmstudioAvailable = await invoke('check_lmstudio_available', { customDir: null });
    console.log('LM Studio available:', state.lmstudioAvailable, 'at', state.lmstudioCacheDir);
    if (state.lmstudioAvailable) {
      state.lmstudioModels = await invoke('get_lmstudio_models', { customDir: null });
      console.log('Found LM Studio models:', state.lmstudioModels.length, state.lmstudioModels);
    }
  } catch (e) {
    console.log('LM Studio check failed:', e);
    state.lmstudioAvailable = false;
    state.lmstudioModels = [];
    state.lmstudioCacheDir = DEFAULT_CACHE_DIR;
  }
}

// ============================================================================
// Render Functions
// ============================================================================

export function renderModelCard(formatBytes, renderCurrentStep) {
  const isActive = state.modelSource === PROVIDER_ID;
  const hasSelection = state.selectedLMStudioModel !== null;

  // If LM Studio is selected and we have a model, show the expanded state with file selection
  if (isActive && hasSelection) {
    const model = state.selectedLMStudioModel;
    const weightsFiles = model.files.filter(f => f.role === 'weights');
    const mmprojFiles = model.files.filter(f => f.role === 'mmproj');
    const selectedWeightsIdx = state.lmstudioFileSelections.weights || 0;
    const selectedMmprojIdx = state.lmstudioFileSelections.mmproj || 0;

    return `
      <div class="custom-llamafile-card active" >
        <div class="card-icon" style="background: var(--${PROVIDER_COLOR}-dim); color: var(--${PROVIDER_COLOR}); font-size: 18px;">${PROVIDER_ICON}</div>
        <div class="card-info">
          <div class="card-title">Load from ${PROVIDER_NAME}</div>
          <div class="custom-file-list">
            <div class="custom-file-entry">
              <span class="file-role-tag">model</span>
              <span class="custom-path-display">${model.displayName}</span>
            </div>
          </div>
        </div>
        <button class="custom-clear-btn" data-action="clearLMStudioModel" data-stop-propagation="true">Clear</button>
      </div>
      <div class="model-file-config" style="margin-top: 8px;">
        ${weightsFiles.length > 0 ? `
          <div class="file-role-section">
            <div class="file-role-header">
              <span class="file-role-label">Weights</span>
              <span class="quant-badge">${weightsFiles[selectedWeightsIdx]?.filename || 'N/A'}</span>
              <span class="file-size-label">${formatBytes(weightsFiles[selectedWeightsIdx]?.sizeBytes || 0)}</span>
              ${weightsFiles.length > 1 ? `
                <div class="expand-toggle" data-action="toggleLMStudioFileExpand" data-role="weights" data-stop-propagation="true">
                  <span class="expand-icon ${state.expandedLMStudioFiles?.weights ? 'expanded' : ''}">▶</span>
                  <span class="expand-count">${weightsFiles.length}</span>
                </div>
              ` : ''}
            </div>
            ${weightsFiles.length > 1 && state.expandedLMStudioFiles?.weights ? `
              <div class="version-releases">
                ${weightsFiles.map((f, idx) => `
                  <div class="release-row ${idx === selectedWeightsIdx ? 'selected' : ''}"
                       data-action="selectLMStudioFile" data-role="weights" data-index="${idx}">
                    <div class="release-radio"></div>
                    <div class="release-info">
                      <span class="release-version">${f.filename}</span>
                    </div>
                    <span class="release-date">${formatBytes(f.sizeBytes)}</span>
                  </div>
                `).join('')}
              </div>
            ` : ''}
          </div>
        ` : ''}
        ${mmprojFiles.length > 0 ? `
          <div class="file-role-section">
            <div class="file-role-header">
              <span class="file-role-label">Vision Adapter (mmproj)</span>
              <span class="quant-badge">${mmprojFiles[selectedMmprojIdx]?.filename || 'N/A'}</span>
              <span class="file-size-label">${formatBytes(mmprojFiles[selectedMmprojIdx]?.sizeBytes || 0)}</span>
              ${mmprojFiles.length > 1 ? `
                <div class="expand-toggle" data-action="toggleLMStudioFileExpand" data-role="mmproj" data-stop-propagation="true">
                  <span class="expand-icon ${state.expandedLMStudioFiles?.mmproj ? 'expanded' : ''}">▶</span>
                  <span class="expand-count">${mmprojFiles.length}</span>
                </div>
              ` : ''}
            </div>
            ${mmprojFiles.length > 1 && state.expandedLMStudioFiles?.mmproj ? `
              <div class="version-releases">
                ${mmprojFiles.map((f, idx) => `
                  <div class="release-row ${idx === selectedMmprojIdx ? 'selected' : ''}"
                       data-action="selectLMStudioFile" data-role="mmproj" data-index="${idx}">
                    <div class="release-radio"></div>
                    <div class="release-info">
                      <span class="release-version">${f.filename}</span>
                    </div>
                    <span class="release-date">${formatBytes(f.sizeBytes)}</span>
                  </div>
                `).join('')}
              </div>
            ` : ''}
          </div>
        ` : ''}
      </div>
      <button class="custom-add-btn" style="margin-top: 8px;" data-action="expandLMStudioSelection" data-stop-propagation="true">Change model</button>
    `;
  }

  // If LM Studio is selected but no model yet, show the list of available models
  if (isActive && !hasSelection) {
    return `
      <div class="custom-llamafile-card active" >
        <div class="card-icon" style="background: var(--${PROVIDER_COLOR}-dim); color: var(--${PROVIDER_COLOR}); font-size: 18px;">${PROVIDER_ICON}</div>
        <div class="card-info">
          <div class="card-title">Load from ${PROVIDER_NAME}</div>
          <div class="card-desc">Select a model from your ${PROVIDER_NAME} cache. <span style="color: var(--accent);">Note: not all models work with llama.cpp/llamafile.</span></div>
        </div>
        <button class="custom-clear-btn" data-action="clearLMStudioModel" data-stop-propagation="true">Cancel</button>
      </div>
      <div class="download-dir-row" style="margin-top: 8px; flex-wrap: wrap; gap: 4px;">
        <span style="color: var(--text-secondary); font-size: 12px; margin-right: 4px;">${PROVIDER_NAME} cache:</span>
        <input type="text" class="path-input" value="${state.lmstudioCacheDir}"
               data-action="updateLMStudioCacheDir"
               style="flex: 1; min-width: 150px; padding: 4px 8px; font-size: 12px; background: var(--bg-input); border: 1px solid var(--border); border-radius: 4px; color: var(--text-primary);">
        <button class="download-dir-btn" data-action="selectLMStudioCacheDir" data-stop-propagation="true" title="Browse...">📂</button>
      </div>
      <div class="version-releases" style="margin-top: 8px;">
        ${state.lmstudioModels.map(m => `
          <div class="release-row" data-action="selectLMStudioModel" data-dir-name="${m.dirName}">
            <div class="release-radio"></div>
            <div class="release-info">
              <span class="release-version">${m.displayName}</span>
              <span style="color: var(--text-secondary); font-size: 11px; margin-left: 8px;">${m.files.length} file${m.files.length !== 1 ? 's' : ''}</span>
            </div>
            <span class="release-date">${formatBytes(m.totalSizeBytes)}</span>
          </div>
        `).join('')}
        ${state.lmstudioModels.length === 0 ? '<div class="card-desc" style="padding: 12px; color: var(--text-secondary);">No LM Studio models found.<br><span style="font-size: 11px;">Make sure this is an LM Studio cache directory containing <code>models/lmstudio-community/</code>.</span></div>' : ''}
      </div>
    `;
  }

  // Default collapsed state - show the card to click
  return `
    <div class="custom-llamafile-card"
         data-action="selectLMStudioSource">
      <div class="card-icon" style="background: var(--${PROVIDER_COLOR}-dim); color: var(--${PROVIDER_COLOR}); font-size: 18px;">${PROVIDER_ICON}</div>
      <div class="card-info">
        <div class="card-title">Load from ${PROVIDER_NAME}</div>
        <div class="card-desc">Use a model from your ${PROVIDER_NAME} cache. <span style="color: var(--accent);">Note: not all models work with llama.cpp/llamafile.</span></div>
      </div>
      <div class="card-meta" style="color: var(--text-secondary);">${state.lmstudioModels.length} models</div>
    </div>
  `;
}

// ============================================================================
// Action Functions
// ============================================================================

export async function selectSource(selectModel, renderCurrentStep) {
  console.log('selectLMStudioSource called, lmstudioModels:', state.lmstudioModels);

  // Switch to LM Studio mode without selecting a model yet
  state.modelSource = PROVIDER_ID;
  state.selectedModel = null;
  state.selectedOllamaModel = null;
  state.selectedLMStudioModel = null;
  state.customModelFiles = [];
  state.modelFileSelections = {};
  state.expandedModelFiles = {};
  state.lmstudioFileSelections = {};
  state.expandedLMStudioFiles = {};

  // If only one LM Studio model, auto-select it
  if (state.lmstudioModels.length === 1) {
    await selectModel(state.lmstudioModels[0].dirName);
    return;
  }

  renderCurrentStep();
}

export async function selectModel(dirName, syncFromBuildState, renderCurrentStep) {
  console.log('selectLMStudioModel called:', dirName);
  try {
    const customDir = state.lmstudioCacheDir !== DEFAULT_CACHE_DIR ? state.lmstudioCacheDir : null;
    const buildState = await invoke('select_lmstudio_model', { dirName, customDir });
    console.log('select_lmstudio_model returned:', buildState);
    syncFromBuildState(buildState);

    // Clear UI-only state
    state.selectedModel = null;
    state.modelFileSelections = {};
    state.expandedModelFiles = {};
    state.expandedLMStudioFiles = {};
    renderCurrentStep();
  } catch (err) {
    console.error('Failed to select LM Studio model:', err);
  }
}

export async function selectFile(role, idx, syncFromBuildState, renderCurrentStep) {
  console.log('selectLMStudioFile called:', role, idx);
  try {
    const buildState = await invoke('select_lmstudio_file', { role, idx });
    syncFromBuildState(buildState);
    renderCurrentStep();
  } catch (err) {
    console.error('Failed to select LM Studio file:', err);
  }
}

export function toggleFileExpand(role, renderCurrentStep) {
  if (!state.expandedLMStudioFiles) {
    state.expandedLMStudioFiles = {};
  }
  state.expandedLMStudioFiles[role] = !state.expandedLMStudioFiles[role];
  renderCurrentStep();
}

export function expandSelection(renderCurrentStep) {
  // Show the model list again (clear selection but stay in LM Studio mode)
  state.selectedLMStudioModel = null;
  state.lmstudioFileSelections = {};
  state.expandedLMStudioFiles = {};
  renderCurrentStep();
}

export async function clearModel(selectRegistryModel, renderCurrentStep) {
  // Re-select first model if available (this will switch back to registry mode)
  if (state.models.length > 0) {
    await selectRegistryModel(state.models[0].id);
  } else {
    state.modelSource = 'registry';
    state.selectedModel = null;
    state.selectedLMStudioModel = null;
    state.lmstudioFileSelections = {};
    state.expandedLMStudioFiles = {};
    renderCurrentStep();
  }
}

export async function selectCacheDir(renderCurrentStep) {
  try {
    const path = await invoke('select_model_download_dir');
    if (path) {
      await updateCacheDir(path, renderCurrentStep);
    }
  } catch (err) {
    console.error('Failed to select LM Studio cache directory:', err);
  }
}

export async function updateCacheDir(path, renderCurrentStep) {
  if (!path || path.trim() === '') return;

  path = path.trim();
  state.lmstudioCacheDir = path;

  // Rescan for models in the new directory
  try {
    state.lmstudioModels = await invoke('get_lmstudio_models', { customDir: path });
    state.lmstudioAvailable = state.lmstudioModels.length > 0;
    console.log('Found LM Studio models in custom dir:', state.lmstudioModels.length);
  } catch (e) {
    console.log('Failed to get LM Studio models:', e);
    state.lmstudioModels = [];
    state.lmstudioAvailable = false;
  }
  // Clear any previous selection since models list changed
  state.selectedLMStudioModel = null;
  state.lmstudioFileSelections = {};
  state.expandedLMStudioFiles = {};
  renderCurrentStep();
}

// ============================================================================
// Validation & Utilities
// ============================================================================

export function isValid() {
  if (state.modelSource !== PROVIDER_ID) return false;
  if (!state.selectedLMStudioModel) return false;

  // Must have at least a weights file selected
  const weightsFiles = state.selectedLMStudioModel.files.filter(f => f.role === 'weights');
  return weightsFiles.length > 0;
}

export function isAvailable() {
  return state.lmstudioAvailable || state.modelSource === PROVIDER_ID;
}

export function getModelSummary() {
  if (!state.selectedLMStudioModel) return `${PROVIDER_NAME} — N/A`;
  return `${PROVIDER_NAME} — ${state.selectedLMStudioModel.displayName}`;
}

export function getSelectedFilename() {
  if (!state.selectedLMStudioModel) return null;

  const model = state.selectedLMStudioModel;
  const weightsIdx = state.lmstudioFileSelections.weights || 0;
  const weightsFile = model.files.filter(f => f.role === 'weights')[weightsIdx];
  return weightsFile?.filename || null;
}

export function getTotalSize() {
  if (!state.selectedLMStudioModel) return 0;

  const model = state.selectedLMStudioModel;
  const weightsIdx = state.lmstudioFileSelections.weights || 0;
  const mmprojIdx = state.lmstudioFileSelections.mmproj || 0;

  const weightsFiles = model.files.filter(f => f.role === 'weights');
  const mmprojFiles = model.files.filter(f => f.role === 'mmproj');

  let total = weightsFiles[weightsIdx]?.sizeBytes || 0;
  if (mmprojFiles.length > 0) {
    total += mmprojFiles[mmprojIdx]?.sizeBytes || 0;
  }
  return total;
}
