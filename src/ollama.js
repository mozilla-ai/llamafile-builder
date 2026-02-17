// llamafile Builder - Ollama Model Provider
// Handles loading and selecting models from local Ollama cache

import { state } from './state.js';
import { invoke } from './tauri.js';

// ============================================================================
// Provider Configuration
// ============================================================================

export const PROVIDER_ID = 'ollama';
export const PROVIDER_NAME = 'Ollama';
export const PROVIDER_ICON = '🦙';
export const PROVIDER_COLOR = 'purple';
export const DEFAULT_CACHE_DIR = '~/.ollama';

// ============================================================================
// State Keys (added to main state)
// ============================================================================

export const stateKeys = {
  available: 'ollamaAvailable',
  models: 'ollamaModels',
  selectedModel: 'selectedOllamaModel',
  cacheDir: 'ollamaCacheDir',
};

// ============================================================================
// Default State Values
// ============================================================================

export const defaultState = {
  ollamaAvailable: false,
  ollamaModels: [],
  selectedOllamaModel: null,
  ollamaCacheDir: '',
};

// ============================================================================
// Initialization
// ============================================================================

export async function init() {
  try {
    state.ollamaCacheDir = await invoke('get_default_ollama_cache_dir');
    state.ollamaAvailable = await invoke('check_ollama_available', { customDir: null });
    console.log('Ollama available:', state.ollamaAvailable, 'at', state.ollamaCacheDir);
    if (state.ollamaAvailable) {
      state.ollamaModels = await invoke('get_ollama_models', { customDir: null });
      console.log('Found Ollama models:', state.ollamaModels.length, state.ollamaModels);
    }
  } catch (e) {
    console.log('Ollama check failed:', e);
    state.ollamaAvailable = false;
    state.ollamaModels = [];
    state.ollamaCacheDir = DEFAULT_CACHE_DIR;
  }
}

// ============================================================================
// Render Functions
// ============================================================================

export function renderModelCard(formatBytes, renderCurrentStep) {
  const isActive = state.modelSource === PROVIDER_ID;
  const hasSelection = state.selectedOllamaModel !== null;

  // If Ollama is selected and we have a model, show the expanded state
  if (isActive && hasSelection) {
    return `
      <div class="custom-llamafile-card active" >
        <div class="card-icon" style="background: var(--${PROVIDER_COLOR}-dim); color: var(--${PROVIDER_COLOR}); font-size: 18px;">${PROVIDER_ICON}</div>
        <div class="card-info">
          <div class="card-title">Load from ${PROVIDER_NAME}</div>
          <div class="custom-file-list">
            <div class="custom-file-entry">
              <span class="file-role-tag">model</span>
              <span class="custom-path-display" title="${state.selectedOllamaModel.weightsPath}">${state.selectedOllamaModel.displayName}</span>
              <span class="file-size-label">${formatBytes(state.selectedOllamaModel.sizeBytes)}</span>
            </div>
          </div>
          <button class="custom-add-btn" data-action="expandOllamaSelection" data-stop-propagation="true">Change model</button>
        </div>
        <button class="custom-clear-btn" data-action="clearOllamaModel" data-stop-propagation="true">Clear</button>
      </div>
    `;
  }

  // If Ollama is selected but no model yet, show the list of available models
  if (isActive && !hasSelection) {
    return `
      <div class="custom-llamafile-card active" >
        <div class="card-icon" style="background: var(--${PROVIDER_COLOR}-dim); color: var(--${PROVIDER_COLOR}); font-size: 18px;">${PROVIDER_ICON}</div>
        <div class="card-info">
          <div class="card-title">Load from ${PROVIDER_NAME}</div>
          <div class="card-desc">Select a model from your local ${PROVIDER_NAME} cache. <span style="color: var(--accent);">Note: not all ${PROVIDER_NAME} models work with llama.cpp/llamafile.</span></div>
        </div>
        <button class="custom-clear-btn" data-action="clearOllamaModel" data-stop-propagation="true">Cancel</button>
      </div>
      <div class="download-dir-row" style="margin-top: 8px; flex-wrap: wrap; gap: 4px;">
        <span style="color: var(--text-secondary); font-size: 12px; margin-right: 4px;">${PROVIDER_NAME} cache:</span>
        <input type="text" class="path-input" value="${state.ollamaCacheDir}"
               data-action="updateOllamaCacheDir"
               style="flex: 1; min-width: 150px; padding: 4px 8px; font-size: 12px; background: var(--bg-input); border: 1px solid var(--border); border-radius: 4px; color: var(--text-primary);">
        <button class="download-dir-btn" data-action="selectOllamaCacheDir" data-stop-propagation="true" title="Browse...">📂</button>
      </div>
      <div class="version-releases" style="margin-top: 8px;">
        ${state.ollamaModels.map(m => `
          <div class="release-row" data-action="selectOllamaModel" data-model-name="${m.modelName}" data-tag="${m.tag}">
            <div class="release-radio"></div>
            <div class="release-info">
              <span class="release-version">${m.displayName}</span>
            </div>
            <span class="release-date">${formatBytes(m.sizeBytes)}</span>
          </div>
        `).join('')}
        ${state.ollamaModels.length === 0 ? '<div class="card-desc" style="padding: 12px; color: var(--text-secondary);">No Ollama models found.<br><span style="font-size: 11px;">Make sure this is an Ollama cache directory containing a <code>models/</code> subfolder.</span></div>' : ''}
      </div>
    `;
  }

  // Default collapsed state - show the card to click
  return `
    <div class="custom-llamafile-card"
         data-action="selectOllamaSource">
      <div class="card-icon" style="background: var(--${PROVIDER_COLOR}-dim); color: var(--${PROVIDER_COLOR}); font-size: 18px;">${PROVIDER_ICON}</div>
      <div class="card-info">
        <div class="card-title">Load from ${PROVIDER_NAME}</div>
        <div class="card-desc">Use a model from your local ${PROVIDER_NAME} cache. <span style="color: var(--accent);">Note: not all ${PROVIDER_NAME} models work with llama.cpp/llamafile.</span></div>
      </div>
      <div class="card-meta" style="color: var(--text-secondary);">${state.ollamaModels.length} models</div>
    </div>
  `;
}

// ============================================================================
// Action Functions
// ============================================================================

export async function selectSource(selectModel, renderCurrentStep) {
  console.log('selectOllamaSource called, ollamaModels:', state.ollamaModels);

  // Switch to Ollama mode without selecting a model yet
  state.modelSource = PROVIDER_ID;
  state.selectedModel = null;
  state.selectedOllamaModel = null;
  state.customModelFiles = [];
  state.modelFileSelections = {};
  state.expandedModelFiles = {};

  // If only one Ollama model, auto-select it
  if (state.ollamaModels.length === 1) {
    await selectModel(state.ollamaModels[0].modelName, state.ollamaModels[0].tag);
    return;
  }

  renderCurrentStep();
}

export async function selectModel(modelName, tag, syncFromBuildState, renderCurrentStep) {
  console.log('selectOllamaModel called:', modelName, tag);
  try {
    const customDir = state.ollamaCacheDir !== DEFAULT_CACHE_DIR ? state.ollamaCacheDir : null;
    const buildState = await invoke('select_ollama_model', { modelName, tag, customDir });
    console.log('select_ollama_model returned:', buildState);
    syncFromBuildState(buildState);

    // Clear UI-only state
    state.selectedModel = null;
    state.modelFileSelections = {};
    state.expandedModelFiles = {};
    renderCurrentStep();
  } catch (err) {
    console.error('Failed to select Ollama model:', err);
  }
}

export function expandSelection(renderCurrentStep) {
  // Show the model list again (clear selection but stay in Ollama mode)
  state.selectedOllamaModel = null;
  renderCurrentStep();
}

export async function clearModel(selectRegistryModel, renderCurrentStep) {
  // Re-select first model if available (this will switch back to registry mode)
  if (state.models.length > 0) {
    await selectRegistryModel(state.models[0].id);
  } else {
    state.modelSource = 'registry';
    state.selectedModel = null;
    state.selectedOllamaModel = null;
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
    console.error('Failed to select Ollama cache directory:', err);
  }
}

export async function updateCacheDir(path, renderCurrentStep) {
  if (!path || path.trim() === '') return;

  path = path.trim();
  state.ollamaCacheDir = path;

  // Rescan for models in the new directory
  try {
    state.ollamaModels = await invoke('get_ollama_models', { customDir: path });
    state.ollamaAvailable = state.ollamaModels.length > 0;
    console.log('Found Ollama models in custom dir:', state.ollamaModels.length);
  } catch (e) {
    console.log('Failed to get Ollama models:', e);
    state.ollamaModels = [];
    state.ollamaAvailable = false;
  }
  // Clear any previous selection since models list changed
  state.selectedOllamaModel = null;
  renderCurrentStep();
}

// ============================================================================
// Validation & Utilities
// ============================================================================

export function isValid() {
  return state.modelSource === PROVIDER_ID && state.selectedOllamaModel !== null;
}

export function isAvailable() {
  return state.ollamaAvailable || state.modelSource === PROVIDER_ID;
}

export function getModelSummary() {
  if (!state.selectedOllamaModel) return `${PROVIDER_NAME} — N/A`;
  return `${PROVIDER_NAME} — ${state.selectedOllamaModel.displayName}`;
}

export function getSelectedFilename() {
  if (!state.selectedOllamaModel) return null;
  return `${state.selectedOllamaModel.displayName.replace(':', '_')}.gguf`;
}

export function getTotalSize() {
  return state.selectedOllamaModel?.sizeBytes || 0;
}
