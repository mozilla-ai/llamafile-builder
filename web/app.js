// ============================================================================
// Global state
// ============================================================================
const S = {
  recipeId: sessionStorage.getItem('recipeId'),
  versions: [],
  models: [],
  schema: [],
  ollamaModels: [],
  lmstudioModels: [],
  state: null,
  resolved: null,      // resolved defaults from /state/resolved
  // UI state
  modelTab: 'registry',
  modelSearch: '',
  modelTag: null,
  expandedGroups: {},
  building: false,
  buildCompleted: false,
  buildTicker: null,
  // Log level filter: 'debug'|'info'|'warn'|'error'
  logLevel: 'info',
  // All log events received since the build started (for client-side filtering).
  allLogs: [],
  // Current build phase label (highest recent tracing::info! message).
  buildPhase: '',
  // filename → { downloaded, total, percentage, done }
  downloads: {},
  // Section-editor modal state.
  modalSection: null,    // section id currently open in modal, or null
  modalSnapshot: null,   // deep copy of SavedState at modal open, for Cancel
  // Inline "add companion file" form toggle (Model section, Local tab).
  addCompanionOpen: false,
  // Latest validation messages so the "N warning(s)" label can open a modal with details.
  validationMessages: [],
};

// ============================================================================
// API helpers
// ============================================================================
async function apiGet(url) {
  const r = await fetch(url);
  if (!r.ok) {
    const e = await r.json().catch(() => ({ error: r.statusText }));
    throw new Error(e.error || r.statusText);
  }
  return r.json();
}

async function apiPost(url, body) {
  const r = await fetch(url, {
    method: 'POST',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!r.ok) {
    const e = await r.json().catch(() => ({ error: r.statusText }));
    throw new Error(e.error || r.statusText);
  }
  return r.json();
}

async function apiPut(url, body) {
  const r = await fetch(url, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const e = await r.json().catch(() => ({ error: r.statusText }));
    throw new Error(e.error || r.statusText);
  }
  return r.json();
}

// ============================================================================
// Utilities
// ============================================================================
function esc(s) {
  return String(s ?? '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function renderDesc(text) {
  if (!text) return '';
  return esc(text).replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
    '<a href="$2" target="_blank" rel="noopener">$1</a>');
}

function fmtBytes(n) {
  if (!n) return '';
  if (n < 1e6) return `${(n/1e3).toFixed(0)} KB`;
  if (n < 1e9) return `${(n/1e6).toFixed(0)} MB`;
  return `${(n/1e9).toFixed(1)} GB`;
}

function getConfigVal(key) {
  // Prefer explicit overrides, fall back to resolved defaults, then schema defaults
  const v = S.state?.config_values?.[key];
  if (v !== undefined) return v;
  const rv = S.resolved?.config_values?.[key];
  if (rv !== undefined) return rv;
  for (const item of S.schema) {
    if (item.key === key) return item.default;
    if (item.type === 'section' && item.items) {
      const found = item.items.find(i => i.key === key);
      if (found) return found.default;
    }
  }
  return null;
}

function findSelectedRelease() {
  if (!S.versions.length) return null;
  const vg = S.state?.version_group || S.resolved?.version_group;
  const vv = S.state?.version || S.resolved?.version;
  if (vg && vv) {
    const g = S.versions.find(g => g.name === vg);
    if (g) return g.releases.find(r => r.version === vv) || null;
  }
  const dg = S.versions.find(g => g.default) || S.versions[0];
  return dg?.releases[0] || null;
}

function getGpuOptions() {
  const rel = findSelectedRelease();
  if (!rel?.gpu_libs) return [];
  const opts = [];
  for (const [accel, osMap] of Object.entries(rel.gpu_libs)) {
    if (!osMap) continue;
    for (const [os, info] of Object.entries(osMap)) {
      if (!info) continue;
      opts.push({ spec: `${accel}:${os}`, accel, os, size: info.size_bytes });
    }
  }
  return opts;
}

function effectiveModelId() {
  return S.state?.model_id || S.resolved?.model_id || null;
}

function selectedModel() {
  const mid = effectiveModelId();
  return mid ? S.models.find(m => m.id === mid) : null;
}

function hasMmproj() {
  const m = selectedModel();
  return !!(m?.files?.find(f => f.role === 'mmproj'));
}

// ============================================================================
// Dynamic sections — mmproj only shown when model has a vision adapter
// ============================================================================
function getVisibleSections() {
  const sections = [
    { id: 'version',  label: 'Version' },
    { id: 'model',    label: 'Model' },
    { id: 'weights',  label: 'Quants' },   // "Weights" renamed to "Quants"
  ];
  if (hasMmproj()) sections.push({ id: 'mmproj', label: 'Mmproj' });
  sections.push(
    { id: 'output',   label: 'Output' },
    { id: 'config',   label: 'Config' },
    { id: 'gpu',      label: 'GPU Libs' },
    { id: 'advanced', label: 'Advanced' },
  );
  return sections;
}

// ============================================================================
// Section summaries — use resolved defaults when state is sparse
// ============================================================================
function getSummary(name) {
  const s = S.state;
  const r = S.resolved;
  if (!s) return '';

  switch (name) {
    case 'version': {
      // Custom runtime: show the full path verbatim — user needs to know exactly what's bundled.
      if (s.custom_runtime) return `custom: ${s.custom_runtime}`;
      const vg = s.version_group || r?.version_group;
      const vv = s.version || r?.version;
      return (vg && vv) ? `${vg} / ${vv}` : '…';
    }
    case 'model': {
      if (s.custom_model_file) {
        // Local: list every file (weights + companions) so the summary is self-describing.
        const files = [`weights: ${s.custom_model_file.split('/').pop()}`];
        for (const [role, path] of Object.entries(s.custom_companion_files || {})) {
          files.push(`${role}: ${path.split('/').pop()}`);
        }
        return `local — ${files.join(', ')}`;
      }
      if (s.ollama_model) return `ollama: ${s.ollama_model.display_name || s.ollama_model.displayName}`;
      if (s.lmstudio_model) return `lm studio: ${s.lmstudio_model.display_name || s.lmstudio_model.displayName}`;
      const mid = s.model_id || r?.model_id;
      const m = mid ? S.models.find(m => m.id === mid) : null;
      return m ? `${m.name} (${m.parameters})` : '…';
    }
    case 'weights': {
      if (!s.model_id && !s.custom_model_file && !s.ollama_model && !s.lmstudio_model) return '—';
      const mid = s.model_id || r?.model_id;
      const m = mid ? S.models.find(m => m.id === mid) : null;
      const wq = s.quantizations?.weights || r?.quantizations?.weights;
      if (!wq) return '—';
      const wf = m?.files?.find(f => f.role === 'weights');
      const q  = wf?.quantizations?.find(q => q.name === wq);
      return q ? `${wq} (${fmtBytes(q.size_bytes)})` : wq;
    }
    case 'mmproj': {
      return (s.quantizations?.mmproj || r?.quantizations?.mmproj) || '—';
    }
    case 'output': {
      const path = s.output_path || r?.output_path;
      if (!path) return '…';
      // Show just the filename component
      return path.split('/').pop() || path;
    }
    case 'config': {
      const cv = Object.assign({}, r?.config_values || {}, s.config_values || {});
      const parts = [];
      // Determine server mode
      if (cv.server_enabled === true) parts.push('mode=server');
      else if (cv.disable_web_ui === true) parts.push('mode=api');
      else parts.push('mode=combined');
      // Context length
      if (cv.context_length !== undefined) parts.push(`ctx=${cv.context_length}`);
      // Temperature
      if (cv.temperature !== undefined) parts.push(`temp=${cv.temperature}`);
      return parts.join('  ') || 'defaults';
    }
    case 'gpu': {
      const specs = s.gpu_libs || [];
      const custom = (s.custom_gpu_libs || []).map(l => l.accel);
      const all = [...specs.map(g => g.split(':')[0]), ...custom];
      return all.length ? [...new Set(all)].join(', ') : 'none';
    }
    case 'advanced': {
      // custom_runtime belongs to the Version section, don't double-count it here.
      const parts = [];
      if (s.custom_zipalign) parts.push('zipalign');
      if (s.args_file) parts.push('.args');
      return parts.length ? `Custom: ${parts.join(', ')}` : 'using defaults';
    }
    default: return '';
  }
}

function hasValue(name) {
  const s = S.state;
  if (!s) return false;
  switch (name) {
    case 'version': return !!(s.version || s.custom_runtime);
    case 'model':   return !!(s.model_id || s.custom_model_file || s.ollama_model || s.lmstudio_model);
    case 'weights': return !!s.quantizations?.weights;
    case 'mmproj':  return !!s.quantizations?.mmproj;
    case 'output':  return !!s.output_path;
    case 'config':  return Object.keys(s.config_values || {}).length > 0;
    case 'gpu':     return (s.gpu_libs || []).length > 0 || (s.custom_gpu_libs || []).length > 0;
    case 'advanced':return !!(s.custom_zipalign || s.args_file);
    default: return false;
  }
}

// ============================================================================
// Init
// ============================================================================
async function init() {
  try {
    // Create or restore recipe
    if (S.recipeId) {
      try { await apiGet(`/api/recipes/${S.recipeId}`); }
      catch (_) { S.recipeId = null; }
    }
    if (!S.recipeId) {
      const r = await apiPost('/api/recipes');
      S.recipeId = r.id;
      sessionStorage.setItem('recipeId', S.recipeId);
    }

    // Load all data + state + resolved in one shot
    const [vers, models, schema, ollama, lmstudio, state, resolved] = await Promise.all([
      apiGet('/api/versions'),
      apiGet('/api/models'),
      apiGet('/api/config-schema'),
      apiGet('/api/ollama/models').catch(() => []),
      apiGet('/api/lmstudio/models').catch(() => []),
      apiGet(`/api/recipes/${S.recipeId}/state`),
      apiGet(`/api/recipes/${S.recipeId}/state/resolved`).catch(() => null),
    ]);
    S.versions = vers; S.models = models; S.schema = schema;
    S.ollamaModels = ollama; S.lmstudioModels = lmstudio;
    S.state = state; S.resolved = resolved;

    renderAll();
    document.getElementById('btn-build').disabled = false;
    await renderRecipeBadge();
    autoValidate();
  } catch (e) {
    document.getElementById('sections').innerHTML =
      `<div class="loading-msg" style="color:var(--red)">Error: ${esc(e.message)}<br><br>` +
      `Is the server running?<br><small style="color:var(--text-dim)">` +
      `llamafile-builder --serve --web-dir web</small></div>`;
  }
}

// Helper: refresh both state and resolved defaults together
async function refreshState() {
  S.state = await apiGet(`/api/recipes/${S.recipeId}/state`);
  S.resolved = await apiGet(`/api/recipes/${S.recipeId}/state/resolved`).catch(() => null);
}

// ============================================================================
// Recipe badge (top-right) — clickable: opens save/load modal.
// Orange when unsaved (no name), green when saved (named).
// ============================================================================
async function renderRecipeBadge() {
  const el = document.getElementById('recipe-badge');
  if (!el) return;
  let recipe = null;
  try { recipe = await apiGet(`/api/recipes/${S.recipeId}`); } catch (_) {}
  const name = recipe?.name;
  const label = name || S.recipeId.slice(0, 8);
  el.textContent = `recipe: ${label}`;
  el.classList.add('clickable');
  el.classList.toggle('saved', !!name);
  el.classList.toggle('unsaved', !name);
  el.title = name
    ? `Saved as "${name}" — click to rename or load another recipe`
    : 'Unsaved draft — click to save or load another recipe';
  el.onclick = showRecipeModal;
}

// Save-and-load modal. Shows the current recipe's name field at the top and a
// list of other saved recipes to switch to below.
async function showRecipeModal() {
  let current = null;
  let all = [];
  try {
    [current, all] = await Promise.all([
      apiGet(`/api/recipes/${S.recipeId}`).catch(() => null),
      apiGet('/api/recipes').catch(() => []),
    ]);
  } catch (_) {}
  const currentName = current?.name || '';

  // Only offer "load" for recipes other than the current one.
  const others = (all || []).filter(r => r.id !== S.recipeId);
  // Named ones first, then drafts, each sorted by updated_at desc.
  others.sort((a, b) => {
    const an = a.name ? 0 : 1, bn = b.name ? 0 : 1;
    if (an !== bn) return an - bn;
    return (b.updated_at || '').localeCompare(a.updated_at || '');
  });

  let loadList = '';
  if (others.length) {
    loadList = '<div class="recipe-load-list">';
    for (const r of others) {
      const label = r.name || r.id.slice(0, 8);
      const cls = r.name ? 'saved' : 'unsaved';
      const model = r.model ? esc(r.model) : '<span style="color:var(--text-dim)">—</span>';
      loadList += `<div class="recipe-load-row">
        <span class="recipe-load-label ${cls}">${esc(label)}</span>
        <span class="recipe-load-model">${model}</span>
        <button class="btn btn-secondary btn-small" data-load-id="${esc(r.id)}">Load</button>
        <button class="btn btn-secondary btn-small btn-danger-hover" data-delete-id="${esc(r.id)}" data-delete-label="${esc(label)}" title="Delete this recipe">✕</button>
      </div>`;
    }
    loadList += '</div>';
  } else {
    loadList = '<div class="empty-state" style="font-size:11px">No other recipes saved yet.</div>';
  }

  const bodyHtml = `
    <div class="recipe-modal-section">
      <div class="recipe-modal-heading">Save current recipe</div>
      <div class="section-desc" style="margin-top:0">
        Naming locks in the current defaults so the recipe stays reproducible
        as the catalog evolves.
      </div>
      <div class="config-row">
        <div class="config-label">Name</div>
        <input class="config-input" id="recipe-name-input" placeholder="my-recipe" value="${esc(currentName)}" autofocus>
        <button class="btn btn-primary btn-small" id="recipe-save-btn">Save</button>
      </div>
    </div>
    <div class="recipe-modal-divider"></div>
    <div class="recipe-modal-section">
      <div class="recipe-modal-heading">Load another recipe</div>
      ${loadList}
    </div>`;

  showModal('Recipes', bodyHtml, null, {
    customFooter: `
      <div style="flex:1"></div>
      <button class="btn btn-secondary" onclick="closeModal()">Close</button>`,
  });

  // Wire save button.
  setTimeout(() => {
    document.getElementById('recipe-save-btn')?.addEventListener('click', saveRecipeFromModal);
    document.getElementById('recipe-name-input')?.focus();
    // Delegate Load clicks.
    document.getElementById('modal-body')?.addEventListener('click', recipeModalClickHandler);
  }, 20);
}

function recipeModalClickHandler(e) {
  const del = e.target.closest('[data-delete-id]');
  if (del) {
    e.stopPropagation();
    deleteRecipeFromModal(del.dataset.deleteId, del.dataset.deleteLabel);
    return;
  }
  const load = e.target.closest('[data-load-id]');
  if (load) {
    const id = load.dataset.loadId;
    if (id) switchToRecipe(id);
  }
}

async function deleteRecipeFromModal(id, label) {
  if (!id) return;
  if (!confirm(`Delete recipe "${label || id.slice(0,8)}"? This cannot be undone.`)) return;
  try {
    const r = await fetch(`/api/recipes/${id}`, { method: 'DELETE' });
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || r.statusText);
    showToast(`Deleted "${label || id.slice(0,8)}"`, 'ok');
    // Re-open the modal so the user sees the updated list without extra clicks.
    showRecipeModal();
  } catch (e) { showToast('Delete failed: ' + e.message, 'err'); }
}

async function saveRecipeFromModal() {
  const name = document.getElementById('recipe-name-input')?.value?.trim();
  if (!name) { showToast('Please enter a name', 'err'); return; }
  try {
    await apiPut(`/api/recipes/${S.recipeId}/name`, { name });
    await refreshState();
    await renderRecipeBadge();
    closeModal();
    showToast(`Saved as "${name}"`, 'ok');
  } catch (e) { showToast('Save failed: ' + e.message, 'err'); }
}

// Switch the active recipe to another one and refresh the whole UI.
async function switchToRecipe(id) {
  S.recipeId = id;
  sessionStorage.setItem('recipeId', id);
  try {
    await refreshState();
    await renderRecipeBadge();
    renderAll();
    autoValidate();
    closeModal();
    showToast('Recipe loaded', 'ok');
  } catch (e) { showToast('Load failed: ' + e.message, 'err'); }
}

// ============================================================================
// Auto-validate — updates build button + status after each mutation
// ============================================================================
async function autoValidate() {
  if (S.building) return;
  try {
    const msgs = await apiGet(`/api/recipes/${S.recipeId}/state/validate`);
    S.validationMessages = msgs;
    const errors   = msgs.filter(m => m.level === 'error');
    const warnings = msgs.filter(m => m.level === 'warning');
    const btn = document.getElementById('btn-build');
    if (errors.length) {
      setStatus(`✗ ${errors.length} error${errors.length > 1 ? 's' : ''}`, 'err');
      if (btn) btn.disabled = true;
    } else if (warnings.length) {
      setStatus(`⚠ ${warnings.length} warning${warnings.length > 1 ? 's' : ''}`, 'warn');
      if (btn) btn.disabled = false;
    } else {
      setStatus('✓ Ready to build', 'ok');
      if (btn) btn.disabled = false;
    }
  } catch (_) {}
}

// Show the latest validation messages in a modal. Wired to the build-status
// element so users can click "⚠ 1 warning" to see what's wrong.
function showValidationDetails() {
  const msgs = S.validationMessages || [];
  if (!msgs.length) return;
  let html = '';
  for (const m of msgs) {
    const cls = m.level === 'error' ? 'err' : 'warn';
    const label = (m.level || '').toUpperCase();
    html += `<div class="validation-row ${cls}">
      <span class="validation-level">${esc(label)}</span>
      <span class="validation-msg">${esc(m.message || '')}</span>
    </div>`;
  }
  showModal('Validation messages', html, null);
}

// ============================================================================
// Native file picker
// ============================================================================
async function browseFile(inputId, mode, title) {
  try {
    const res = await apiPost('/api/file-dialog', { mode, title });
    if (res.path) {
      const inp = document.getElementById(inputId);
      if (inp) {
        inp.value = res.path;
        // Trigger blur to save if it's a path input
        inp.dispatchEvent(new Event('blur', { bubbles: true }));
      }
    }
  } catch (_) { /* server has no display — user types manually */ }
}

// ============================================================================
// Dashboard rendering — each section is a clickable card that opens a modal
// ============================================================================
function renderAll() {
  const SECTIONS = getVisibleSections();
  const container = document.getElementById('sections');
  let html = '';
  for (const sec of SECTIONS) {
    const summary = getSummary(sec.id);
    const dim = ['defaults', 'using defaults', 'none', 'auto', '—', '…'].includes(summary);
    const active = hasValue(sec.id);
    html += `
      <div class="section ${active ? 'has-value' : ''}" id="sec-${sec.id}">
        <div class="section-header" data-open="${sec.id}">
          <span class="section-dot"></span>
          <span class="section-label">${esc(sec.label)}</span>
          <span class="section-summary ${dim ? 'dim' : ''}" id="ss-${sec.id}">${esc(summary)}</span>
          <span class="section-chevron">▶</span>
        </div>
      </div>`;
  }
  container.innerHTML = html;
  bindAll();
}

// Refresh a section's dashboard summary card. If the section is currently open
// in the modal, re-render the modal body too.
function refreshSection(name) {
  const summary = getSummary(name);
  const dim = ['defaults', 'using defaults', 'none', 'auto', '—', '…'].includes(summary);
  const el = document.getElementById(`ss-${name}`);
  if (el) { el.textContent = summary; el.className = `section-summary ${dim ? 'dim' : ''}`; }
  const sec = document.getElementById(`sec-${name}`);
  if (sec) { hasValue(name) ? sec.classList.add('has-value') : sec.classList.remove('has-value'); }

  if (S.modalSection === name) {
    const body = document.getElementById('modal-body');
    if (body) {
      body.innerHTML = getContent(name);
      bindSectionModalBody(name);
    }
  }
}

function getContent(name) {
  switch (name) {
    case 'version':   return renderVersionContent();
    case 'model':     return renderModelContent();
    case 'weights':   return renderWeightsContent();
    case 'mmproj':    return renderMmprojContent();
    case 'output':    return renderOutputContent();
    case 'config':    return renderConfigContent();
    case 'gpu':       return renderGpuContent();
    case 'advanced':  return renderAdvancedContent();
    default:          return '';
  }
}

const SECTION_DESCS = {
  version:  'Select the llamafile runtime version to bundle with your model.',
  model:    'Choose a model from the registry, a local .gguf file, Ollama, or LM Studio.',
  weights:  'Quantization level for model weights — trades size for quality.',
  mmproj:   'Quantization for the vision adapter. Only applies to multimodal models.',
  output:   'Where to save the finished .llamafile. Leave blank for auto-generated path.',
  config:   'Runtime parameters embedded in the llamafile (context window, temperature, server mode).',
  gpu:      'GPU acceleration libraries to bundle. These add significant size but enable CUDA/ROCm acceleration on the target system.',
  advanced: 'Override internal tools or supply a pre-built .args file. For power users.',
};

// ============================================================================
// Dashboard click binding — card click opens section modal
// ============================================================================
let _sectionsHandler = null;

function bindAll() {
  const sections = document.getElementById('sections');
  if (_sectionsHandler) sections.removeEventListener('click', _sectionsHandler);
  _sectionsHandler = function(e) {
    const hdr = e.target.closest('[data-open]');
    if (hdr) { e.stopPropagation(); openSectionModal(hdr.dataset.open); }
  };
  sections.addEventListener('click', _sectionsHandler);
}

// ============================================================================
// Modal section editor — opens a section's content in a modal with OK / Cancel
// ============================================================================
function openSectionModal(name) {
  const SECTIONS = getVisibleSections();
  const sec = SECTIONS.find(s => s.id === name);
  if (!sec) return;

  // Snapshot state for Cancel/revert. Deep copy via JSON round-trip.
  S.modalSection = name;
  S.modalSnapshot = S.state ? JSON.parse(JSON.stringify(S.state)) : null;

  showModal(
    sec.label,
    getContent(name),
    null,
    {
      sectionEditor: true,
      customFooter: `
        <div style="flex:1"></div>
        <button class="btn btn-secondary" onclick="cancelSectionModal()">Cancel</button>
        <button class="btn btn-primary" onclick="confirmSectionModal()">OK</button>`,
    }
  );

  bindSectionModalBody(name);
}

function bindSectionModalBody(name) {
  const body = document.getElementById('modal-body');
  if (!body) return;
  // Remove previous handler if present (single delegate per modal body).
  if (body._sectionHandler) body.removeEventListener('click', body._sectionHandler);
  body._sectionHandler = makeSectionHandler(name);
  body.addEventListener('click', body._sectionHandler);

  // Re-bind input-level listeners for sections that have them.
  if (name === 'model') bindModelSearch();
  if (name === 'output') bindPathInputs();
}

async function confirmSectionModal() {
  S.modalSection = null;
  S.modalSnapshot = null;
  closeModal();
}

async function cancelSectionModal() {
  const snapshot = S.modalSnapshot;
  S.modalSection = null;
  S.modalSnapshot = null;
  closeModal();
  if (!snapshot) return;

  try {
    await apiPut(`/api/recipes/${S.recipeId}/state`, snapshot);
    await refreshState();
    renderAll();
    autoValidate();
  } catch (e) {
    showToast('Revert failed: ' + e.message, 'err');
  }
}

function makeSectionHandler(name) {
  return function(e) {
    switch (name) {
      case 'version':  handleVersionClick(e);  break;
      case 'model':    handleModelClick(e);    break;
      case 'weights':  handleWeightsClick(e);  break;
      case 'mmproj':   handleMmprojClick(e);   break;
      case 'gpu':      handleGpuClick(e);      break;
      case 'output':   handleOutputClick(e);   break;
      case 'advanced': handleAdvancedClick(e); break;
    }
  };
}

function bindModelSearch() {
  const inp = document.getElementById('model-search');
  if (!inp || inp.dataset.searchBound) return;
  inp.dataset.searchBound = '1';
  inp.addEventListener('input', function() {
    S.modelSearch = this.value;
    refreshSection('model');
    const newInp = document.getElementById('model-search');
    if (newInp) { newInp.focus(); newInp.selectionStart = newInp.selectionEnd = S.modelSearch.length; }
  });
}

function bindPathInputs() {
  const inp = document.getElementById('output-path-input');
  if (!inp || inp.dataset.pathBound) return;
  inp.dataset.pathBound = '1';
  inp.addEventListener('blur', function() {
    const val = this.value.trim();
    if (val !== (S.state?.output_path || '')) setOutputPath(val);
  });
  inp.addEventListener('keydown', function(e) {
    if (e.key === 'Enter') { e.preventDefault(); this.blur(); }
  });
}

// ============================================================================
// Version section
// ============================================================================
function renderVersionContent() {
  if (!S.versions.length) return '<div class="empty-state">Loading...</div>';
  const s = S.state;
  const r = S.resolved;
  const effGroup = s?.version_group || r?.version_group;
  const effVer   = s?.version || r?.version;

  let html = `<div class="section-desc">${SECTION_DESCS.version}</div>`;

  for (const group of S.versions) {
    const groupSel = group.name === effGroup;
    const expanded = S.expandedGroups[group.name] ?? groupSel;

    html += `<div class="version-group">
      <div class="selection-card ${groupSel ? 'selected' : ''}" data-vgroup="${esc(group.name)}">
        <div class="radio-dot" data-vgroup-select="${esc(group.name)}"></div>
        <div class="card-info">
          <div class="card-title">${esc(group.name)}</div>
          <div class="card-desc">${renderDesc(group.description)}</div>
        </div>
        ${group.tag ? `<div class="card-badge badge-${esc(group.tag)}">${esc(group.tag)}</div>` : ''}
        <div class="expand-toggle" data-vgroup-toggle="${esc(group.name)}">
          <span class="expand-icon ${expanded ? 'expanded' : ''}">▶</span>
          <span>${group.releases.length}</span>
        </div>
      </div>`;

    if (expanded) {
      html += '<div class="version-releases">';
      for (const rel of group.releases) {
        const sel = rel.version === effVer && group.name === effGroup;
        html += `<div class="release-row ${sel ? 'selected' : ''}"
            data-vgroup="${esc(group.name)}" data-vrel="${esc(rel.version)}">
          <div class="release-radio"></div>
          <div class="release-version">${esc(rel.version)}</div>
          <div class="release-desc">${renderDesc(rel.description)}</div>
          <div class="release-date">${esc(rel.release_date || '')}</div>
        </div>`;
      }
      html += '</div>';
    }
    html += '</div>';
  }

  const customRT = s?.custom_runtime || '';
  html += `<div class="config-row" style="margin-top:14px">
    <div class="config-label" style="font-size:11px;color:var(--text-dim)">Custom runtime</div>
    <input class="config-input" id="custom-runtime-input" placeholder="/path/to/llamafile" value="${esc(customRT)}">
    <button class="btn btn-secondary btn-small btn-browse" onclick="browseFile('custom-runtime-input','open','Select llamafile runtime')">Browse…</button>
    <button class="btn btn-secondary btn-small" data-action="set-runtime">Set</button>
  </div>`;
  return html;
}

function toggleVersionGroup(name) {
  const group = S.versions.find(g => g.name === name);
  const effGroup = S.state?.version_group || S.resolved?.version_group;
  const cur = S.expandedGroups[name] ?? (group?.name === effGroup);
  S.expandedGroups[name] = !cur;
  refreshSection('version');
}

function handleVersionClick(e) {
  // Click on radio-dot → select the most recent release in that group
  const selectDot = e.target.closest('[data-vgroup-select]');
  if (selectDot) {
    e.stopPropagation();
    const gname = selectDot.dataset.vgroupSelect;
    const group = S.versions.find(g => g.name === gname);
    if (group?.releases?.[0]) selectVersion(gname, group.releases[0].version);
    return;
  }
  // Click on expand toggle
  const toggle = e.target.closest('[data-vgroup-toggle]');
  if (toggle) {
    e.stopPropagation();
    toggleVersionGroup(toggle.dataset.vgroupToggle);
    return;
  }
  // Click on release row
  const rel = e.target.closest('[data-vrel]');
  if (rel) { selectVersion(rel.dataset.vgroup, rel.dataset.vrel); return; }
  // Click on group card (body) → toggle expand
  const card = e.target.closest('[data-vgroup]');
  if (card) { toggleVersionGroup(card.dataset.vgroup); return; }
  // Custom runtime set
  if (e.target.closest('[data-action="set-runtime"]')) {
    const val = document.getElementById('custom-runtime-input')?.value?.trim();
    if (val) setCustomRuntime(val);
  }
}

async function selectVersion(group, version) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/version`, { group, version });
    await refreshState();
    S.expandedGroups[group] = true;
    refreshSection('version');
    refreshSection('gpu');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// Model section
// ============================================================================
function renderModelContent() {
  const { state: s, modelTab, modelSearch, modelTag } = S;

  let filtered = S.models;
  if (modelSearch) {
    const q = modelSearch.toLowerCase();
    filtered = filtered.filter(m =>
      m.name.toLowerCase().includes(q) || m.id.toLowerCase().includes(q) ||
      (m.organization || '').toLowerCase().includes(q)
    );
  }
  if (modelTag) filtered = filtered.filter(m => m.tags?.includes(modelTag));

  const allTags = [...new Set(S.models.flatMap(m => m.tags || []))].sort();
  const effModelId = effectiveModelId();

  let html = `<div class="section-desc">${SECTION_DESCS.model}</div>`;

  const tabs = [
    { id: 'registry', label: 'Registry' },
    { id: 'ollama',   label: 'Ollama' },
    { id: 'lmstudio', label: 'LM Studio' },
    { id: 'file',     label: 'Local File' },
  ];
  html += '<div class="tab-bar">';
  for (const t of tabs) html += `<div class="tab ${modelTab === t.id ? 'active' : ''}" data-tab="${t.id}">${t.label}</div>`;
  html += '</div>';

  if (modelTab === 'registry') {
    html += `<div class="search-bar">
      <span class="search-icon">⌕</span>
      <input class="search-input" id="model-search" placeholder="Search models…"
        value="${esc(modelSearch)}" autocomplete="off" spellcheck="false">
    </div>`;
    if (allTags.length) {
      html += '<div class="filter-row">';
      html += `<div class="filter-chip ${!modelTag ? 'active' : ''}" data-tag="">All</div>`;
      for (const tag of allTags) html += `<div class="filter-chip ${modelTag === tag ? 'active' : ''}" data-tag="${esc(tag)}">${esc(tag)}</div>`;
      html += '</div>';
    }
    if (!filtered.length) {
      html += '<div class="empty-state">No models match your search.</div>';
    } else {
      for (const m of filtered) {
        const sel = m.id === effModelId;
        const wf = m.files?.find(f => f.role === 'weights');
        const size = wf?.quantizations?.[0]?.size_bytes;
        const tags = (m.tags || []).map(t => `<span class="tag-chip ${t}">${esc(t)}</span>`).join(' ');
        html += `<div class="selection-card ${sel ? 'selected' : ''}" data-model-id="${esc(m.id)}">
          <div class="radio-dot"></div>
          <div class="card-info">
            <div class="card-title">${esc(m.name)} ${tags}</div>
            <div class="card-desc">${esc(m.description || '')}${m.parameters ? ' · ' + esc(m.parameters) : ''}</div>
          </div>
          ${size ? `<div class="card-meta">${fmtBytes(size)}</div>` : ''}
        </div>`;
      }
    }
  } else if (modelTab === 'ollama') {
    if (!S.ollamaModels.length) {
      html += '<div class="empty-state">No Ollama models found.<br><span style="color:var(--text-dim)">Is Ollama running?</span></div>';
    } else {
      for (const m of S.ollamaModels) {
        const name = m.display_name || m.displayName;
        const cur = s?.ollama_model?.display_name || s?.ollama_model?.displayName;
        html += `<div class="selection-card ${cur === name ? 'selected' : ''}" data-ollama="${esc(name)}">
          <div class="radio-dot"></div>
          <div class="card-info">
            <div class="card-title">${esc(name)}</div>
            <div class="card-desc">${esc(m.path || m.dir_path || '')}</div>
          </div>
        </div>`;
      }
    }
  } else if (modelTab === 'lmstudio') {
    if (!S.lmstudioModels.length) {
      html += '<div class="empty-state">No LM Studio models found.</div>';
    } else {
      for (const m of S.lmstudioModels) {
        const name = m.display_name || m.displayName;
        const dir  = m.dir_name || m.dirName;
        const cur  = s?.lmstudio_model?.display_name || s?.lmstudio_model?.displayName;
        html += `<div class="selection-card ${cur === name ? 'selected' : ''}" data-lmstudio="${esc(dir || name)}">
          <div class="radio-dot"></div>
          <div class="card-info">
            <div class="card-title">${esc(name)}</div>
            <div class="card-desc">${esc(dir || '')}</div>
          </div>
        </div>`;
      }
    }
  } else if (modelTab === 'file') {
    html += renderLocalFilesTab(s);
  }

  html += '';
  return html;
}

// ============================================================================
// Local file tab — multi-file UI (weights + optional companions like mmproj)
// ============================================================================
function renderLocalFilesTab(s) {
  const weightsPath = s?.custom_model_file || '';
  const companions = s?.custom_companion_files || {};
  const addOpen = !!S.addCompanionOpen;

  let html = `<div class="section-desc" style="margin-top:0">
    Load local .gguf files from disk. The first file is the weights (required);
    add an mmproj for multimodal models.
  </div>`;

  if (weightsPath) {
    html += `<div class="local-files-list">
      <div class="local-file-row">
        <span class="role-badge role-weights">WEIGHTS</span>
        <span class="local-file-name" title="${esc(weightsPath)}">${esc(weightsPath.split('/').pop())}</span>
        <button class="btn btn-secondary btn-small" data-action="clear-model-file">✕</button>
      </div>`;
    for (const [role, path] of Object.entries(companions)) {
      html += `<div class="local-file-row">
        <span class="role-badge role-${esc(role)}">${esc(role.toUpperCase())}</span>
        <span class="local-file-name" title="${esc(path)}">${esc(path.split('/').pop())}</span>
        <button class="btn btn-secondary btn-small" data-remove-companion="${esc(role)}">✕</button>
      </div>`;
    }
    html += `</div>
    <div class="local-file-actions">
      <button class="btn btn-secondary btn-small" data-action="toggle-add-companion">${addOpen ? '— Cancel add' : '+ Add file'}</button>
      <button class="btn btn-secondary btn-small btn-danger-hover" data-action="clear-model-file">Clear all</button>
    </div>`;

    // Inline add-companion form (does not open a nested modal).
    if (addOpen) {
      html += `<div class="companion-add-form">
        <div class="config-row">
          <div class="config-label">Role</div>
          <select class="config-select" id="companion-role-select">
            <option value="mmproj">mmproj (vision adapter)</option>
            <option value="custom">custom…</option>
          </select>
          <input class="config-input" id="companion-role-custom" placeholder="role name" style="display:none">
        </div>
        <div class="path-row" style="margin-top:8px">
          <input class="path-input" id="companion-path-input" placeholder="/path/to/mmproj.gguf">
          <button class="btn btn-secondary btn-browse btn-small" onclick="browseFile('companion-path-input','open','Select companion file')">Browse…</button>
          <button class="btn btn-primary btn-small" data-action="submit-companion">Add</button>
        </div>
      </div>`;
    }
  } else {
    html += `<div class="path-row">
      <input class="path-input" id="model-file-input" placeholder="/path/to/model.gguf">
      <button class="btn btn-secondary btn-browse" onclick="browseAndSetModelFile()">Browse…</button>
      <button class="btn btn-secondary btn-small" data-action="set-model-file">Use</button>
    </div>`;
  }
  return html;
}

function handleModelClick(e) {
  const tab = e.target.closest('[data-tab]');
  if (tab) { S.modelTab = tab.dataset.tab; refreshSection('model'); return; }
  const chip = e.target.closest('.filter-chip');
  if (chip) { S.modelTag = chip.dataset.tag || null; refreshSection('model'); return; }
  const modelCard = e.target.closest('[data-model-id]');
  if (modelCard) { selectModel(modelCard.dataset.modelId); return; }
  const ollamaCard = e.target.closest('[data-ollama]');
  if (ollamaCard) { selectOllamaModel(ollamaCard.dataset.ollama); return; }
  const lmsCard = e.target.closest('[data-lmstudio]');
  if (lmsCard) { selectLmStudioModel(lmsCard.dataset.lmstudio); return; }

  // Local file actions
  if (e.target.closest('[data-action="set-model-file"]')) {
    const val = document.getElementById('model-file-input')?.value?.trim();
    if (val) setModelFile(val);
    return;
  }
  if (e.target.closest('[data-action="clear-model-file"]')) {
    S.addCompanionOpen = false;
    clearModelFiles();
    return;
  }
  if (e.target.closest('[data-action="toggle-add-companion"]')) {
    S.addCompanionOpen = !S.addCompanionOpen;
    refreshSection('model');
    // Wire up the custom role field visibility when the form opens.
    if (S.addCompanionOpen) {
      setTimeout(() => {
        const sel = document.getElementById('companion-role-select');
        const input = document.getElementById('companion-role-custom');
        sel?.addEventListener('change', () => {
          if (input) input.style.display = sel.value === 'custom' ? '' : 'none';
        });
      }, 0);
    }
    return;
  }
  if (e.target.closest('[data-action="submit-companion"]')) {
    submitCompanionAdd();
    return;
  }
  const removeBtn = e.target.closest('[data-remove-companion]');
  if (removeBtn) {
    removeCompanionFile(removeBtn.dataset.removeCompanion);
  }
}

// Submit the inline companion-add form — stays inside the Model section modal.
async function submitCompanionAdd() {
  const sel = document.getElementById('companion-role-select')?.value;
  const role = sel === 'custom'
    ? (document.getElementById('companion-role-custom')?.value?.trim() || 'companion')
    : sel;
  const path = document.getElementById('companion-path-input')?.value?.trim();
  if (!path) { showToast('Please enter a file path', 'err'); return; }
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/companion-file`, { action: 'add', role, path });
    S.addCompanionOpen = false;
    await refreshState();
    refreshSection('model');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function browseAndSetModelFile() {
  try {
    const res = await apiPost('/api/file-dialog', { mode: 'open', title: 'Select model weights (.gguf)' });
    if (res.path) {
      const inp = document.getElementById('model-file-input');
      if (inp) inp.value = res.path;
      await setModelFile(res.path);
    }
  } catch (_) {
    // No display — user types manually
  }
}

async function clearModelFiles() {
  try {
    // Clear companion files first, then weights
    for (const role of Object.keys(S.state?.custom_companion_files || {})) {
      await apiPost(`/api/recipes/${S.recipeId}/state/companion-file`, { action: 'remove', role });
    }
    await apiPost(`/api/recipes/${S.recipeId}/state/model-file`, { path: '' });
    await refreshState();
    renderAll();
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function removeCompanionFile(role) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/companion-file`, { action: 'remove', role });
    await refreshState();
    refreshSection('model');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function selectModel(id) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/model`, { id });
    await refreshState();
    renderAll();
    // Re-render the open Model modal so the selection is reflected.
    if (S.modalSection === 'model') refreshSection('model');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function selectOllamaModel(name) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/ollama-model`, { name });
    await refreshState();
    renderAll();
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function selectLmStudioModel(name) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/lmstudio-model`, { name });
    await refreshState();
    renderAll();
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function setModelFile(path) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/model-file`, { path });
    await refreshState();
    renderAll();
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// Weights section
// ============================================================================
function renderWeightsContent() {
  const s = S.state;
  const m = selectedModel();

  if (!m && !s?.model_id) {
    if (s?.custom_model_file || s?.ollama_model || s?.lmstudio_model)
      return `<div class="section-desc">${SECTION_DESCS.weights}</div><div class="empty-state">Quantization is only available for registry models.</div>`;
    return `<div class="section-desc">${SECTION_DESCS.weights}</div><div class="empty-state">Select a registry model first.</div>`;
  }
  if (!m) return `<div class="section-desc">${SECTION_DESCS.weights}</div><div class="empty-state">Loading…</div>`;

  const wf = m.files?.find(f => f.role === 'weights');
  if (!wf?.quantizations?.length)
    return `<div class="section-desc">${SECTION_DESCS.weights}</div><div class="empty-state">No quantization options available.</div>`;

  const curQ = s?.quantizations?.weights || S.resolved?.quantizations?.weights;
  let html = `<div class="section-desc">${SECTION_DESCS.weights}</div><div class="quant-grid">`;
  for (const q of wf.quantizations) {
    const sel = curQ ? q.name === curQ : q === wf.quantizations[0];
    html += `<div class="quant-card ${sel ? 'selected' : ''}" data-quant-role="weights" data-quant-name="${esc(q.name)}">
      <div class="quant-name">${esc(q.name)}</div>
      ${q.size_bytes ? `<div class="quant-size">${fmtBytes(q.size_bytes)}</div>` : ''}
    </div>`;
  }
  html += '</div>';
  if (!s?.quantizations?.weights) html += `<div class="quant-hint">Default highlighted — click to explicitly set.</div>`;
  return html;
}

function handleWeightsClick(e) {
  const card = e.target.closest('[data-quant-role="weights"]');
  if (card) selectQuant('weights', card.dataset.quantName);
}

// ============================================================================
// Mmproj section
// ============================================================================
function renderMmprojContent() {
  const s = S.state;
  const m = selectedModel();

  if (!m) return `<div class="section-desc">${SECTION_DESCS.mmproj}</div><div class="empty-state">Select a registry model with a vision adapter.</div>`;
  const mf = m.files?.find(f => f.role === 'mmproj');
  if (!mf?.quantizations?.length)
    return `<div class="section-desc">${SECTION_DESCS.mmproj}</div><div class="empty-state">This model has no vision adapter (mmproj).</div>`;

  const curQ = s?.quantizations?.mmproj || S.resolved?.quantizations?.mmproj;
  let html = `<div class="section-desc">${SECTION_DESCS.mmproj}</div><div class="quant-grid">`;
  for (const q of mf.quantizations) {
    const sel = curQ ? q.name === curQ : q === mf.quantizations[0];
    html += `<div class="quant-card ${sel ? 'selected' : ''}" data-quant-role="mmproj" data-quant-name="${esc(q.name)}">
      <div class="quant-name">${esc(q.name)}</div>
      ${q.size_bytes ? `<div class="quant-size">${fmtBytes(q.size_bytes)}</div>` : ''}
    </div>`;
  }
  html += '</div>';
  if (!s?.quantizations?.mmproj) html += `<div class="quant-hint">Default highlighted — click to explicitly set.</div>`;
  return html;
}

function handleMmprojClick(e) {
  const card = e.target.closest('[data-quant-role="mmproj"]');
  if (card) selectQuant('mmproj', card.dataset.quantName);
}

async function selectQuant(role, name) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/quantization`, { role, name });
    await refreshState();
    refreshSection(role === 'weights' ? 'weights' : 'mmproj');
    // Output summary will also update (path is derived from quant filename)
    refreshSection('output');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// Output section
// ============================================================================
function renderOutputContent() {
  const s = S.state;
  const autoPath = S.resolved?.output_path || '';
  const customPath = s?.output_path || '';

  let html = `<div class="section-desc">${SECTION_DESCS.output}</div>`;
  html += `<div class="path-row">
    <input class="path-input" id="output-path-input"
      placeholder="${esc(autoPath) || '~/Desktop/model.llamafile'}"
      value="${esc(customPath)}">
    <button class="btn btn-secondary btn-browse" onclick="browseFile('output-path-input','save','Save llamafile as…')">Browse…</button>
    ${customPath ? `<button class="btn btn-secondary btn-small" data-action="clear-output">Clear</button>` : ''}
  </div>`;
  if (autoPath && !customPath) {
    html += `<div class="inline-note">Auto-generated from model: ${esc(autoPath)}</div>`;
  }
  return html;
}

function handleOutputClick(e) {
  if (e.target.closest('[data-action="clear-output"]')) setOutputPath('');
}

async function setOutputPath(path) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/output`, { path });
    await refreshState();
    refreshSection('output');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// Config section (schema-driven)
// ============================================================================
function renderConfigContent() {
  if (!S.schema.length) return `<div class="section-desc">${SECTION_DESCS.config}</div><div class="empty-state">Loading schema…</div>`;

  const hasArgsOverride = !!(S.state?.args_file);
  let html = `<div class="section-desc">${SECTION_DESCS.config}</div>`;
  if (hasArgsOverride) {
    html += `<div class="warn-note">⚠ Custom .args file is active — these settings are informational only. Use "Edit .args" to modify.</div>`;
  }
  for (const item of S.schema) {
    if (item.hidden) continue;
    if (item.type === 'section') {
      html += `<div class="config-group"><div class="config-group-title">${esc(item.label)}</div>`;
      for (const sub of (item.items || [])) {
        if (sub.hidden) continue;
        html += renderConfigItem(sub);
      }
      html += '</div>';
    } else {
      html += renderConfigItem(item);
    }
  }
  return html;
}

function renderConfigItem(item) {
  const cur = getConfigVal(item.key);
  if (item.type === 'bool') {
    const on = cur !== null && cur !== undefined ? !!cur : !!item.default;
    return `<div class="toggle-row" data-bool-key="${esc(item.key)}">
      <div class="toggle-info">
        <div class="toggle-title">${esc(item.label)}</div>
        ${item.description ? `<div class="toggle-desc">${esc(item.description)}</div>` : ''}
      </div>
      <div class="toggle-switch ${on ? 'on' : ''}"></div>
    </div>`;
  }
  if (item.type === 'dropdown') {
    const val = cur !== null ? cur : item.default;
    let opts = '';
    for (const opt of (item.options || [])) {
      const display = opt === 0 ? 'model max' : opt;
      opts += `<option value="${esc(opt)}" ${String(val) === String(opt) ? 'selected' : ''}>${esc(display)}</option>`;
    }
    return `<div class="config-row">
      <div class="config-label">${esc(item.label)}</div>
      <select class="config-select" data-config-key="${esc(item.key)}" data-config-type="number">${opts}</select>
    </div>`;
  }
  if (item.type === 'slider') {
    const val = cur !== null && cur !== undefined ? cur : item.default;
    return `<div class="config-row">
      <div class="config-label">${esc(item.label)}</div>
      <div class="slider-wrap">
        <input type="range" class="config-slider"
          min="${esc(item.min)}" max="${esc(item.max)}" step="${esc(item.step)}"
          value="${esc(val)}" data-config-key="${esc(item.key)}" data-config-type="number"
          oninput="this.nextElementSibling.textContent=this.value">
        <span class="slider-val">${esc(val)}</span>
      </div>
    </div>`;
  }
  const isNum = item.value_type === 'number';
  const val = cur !== null && cur !== undefined ? cur : (item.default || '');
  return `<div class="config-row">
    <div class="config-label">${esc(item.label)}</div>
    <input class="config-input" type="${isNum ? 'number' : 'text'}"
      value="${esc(val)}" placeholder="${esc(item.default || '')}"
      data-config-key="${esc(item.key)}" data-config-type="${isNum ? 'number' : 'string'}">
  </div>`;
}

document.addEventListener('click', async function(e) {
  const boolRow = e.target.closest('[data-bool-key]');
  if (boolRow) {
    const key = boolRow.dataset.boolKey;
    const sw = boolRow.querySelector('.toggle-switch');
    const cur = sw?.classList.contains('on');
    await setConfigVal(key, !cur);
  }
});

document.addEventListener('change', async function(e) {
  const key = e.target.dataset?.configKey;
  if (!key) return;
  const type = e.target.dataset.configType;
  let val;
  if (type === 'number') { val = Number(e.target.value); if (isNaN(val)) return; }
  else { val = e.target.value; }
  await setConfigVal(key, val);
});

async function setConfigVal(key, val) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/config`, { key, value: val });
    await refreshState();
    refreshSection('config');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// GPU section
// ============================================================================
function renderGpuContent() {
  const opts = getGpuOptions();
  const current = new Set(S.state?.gpu_libs || []);
  const customLibs = S.state?.custom_gpu_libs || [];
  const accelLabel = { cuda: 'CUDA — NVIDIA', rocm: 'ROCm — AMD', vulkan: 'Vulkan', metal: 'Metal — Apple' };
  const osLabel    = { linux: 'Linux (.so)', windows: 'Windows (.dll)', macos: 'macOS (.dylib)' };

  let html = `<div class="section-desc">${SECTION_DESCS.gpu}</div>`;

  if (!opts.length) {
    html += '<div class="empty-state">No GPU libraries available for the selected version.</div>';
  } else {
    html += '<div class="gpu-list">';
    for (const opt of opts) {
      const checked = current.has(opt.spec);
      html += `<div class="gpu-row ${checked ? 'checked' : ''}" data-gpu-spec="${esc(opt.spec)}">
        <div class="gpu-check">${checked ? '✓' : ''}</div>
        <div class="gpu-info">
          <div class="gpu-name">${esc(accelLabel[opt.accel] || opt.accel)}</div>
          <div class="gpu-meta">${esc(osLabel[opt.os] || opt.os)}${opt.size ? ' · ' + fmtBytes(opt.size) : ''}</div>
        </div>
      </div>`;
    }
    html += '</div>';
  }

  // Custom GPU lib files
  html += `<div class="gpu-custom-section">
    <div class="config-group-title">Custom library files</div>`;
  if (customLibs.length) {
    html += '<div class="gpu-custom-list">';
    for (const lib of customLibs) {
      html += `<div class="gpu-custom-item">
        <span title="${esc(lib.path)}">${esc(lib.path.split('/').pop())} · ${esc(lib.accel)}:${esc(lib.os)}</span>
        <button class="btn btn-secondary btn-small" data-remove-gpu="${esc(lib.path)}">✕</button>
      </div>`;
    }
    html += '</div>';
  }
  html += `<div class="gpu-add-form">
    <input class="config-input" id="gpu-custom-path" placeholder="/path/to/ggml-cuda.so" style="flex:1;min-width:160px">
    <button class="btn btn-secondary btn-browse btn-small" onclick="browseFile('gpu-custom-path','open','Select GPU library file')">Browse…</button>
    <select class="gpu-add-select" id="gpu-custom-accel">
      <option value="cuda">cuda</option>
      <option value="rocm">rocm</option>
      <option value="vulkan">vulkan</option>
    </select>
    <select class="gpu-add-select" id="gpu-custom-os">
      <option value="linux">linux</option>
      <option value="windows">windows</option>
    </select>
    <button class="btn btn-secondary btn-small" data-action="add-custom-gpu">Add</button>
  </div></div>`;
  return html;
}

function handleGpuClick(e) {
  const row = e.target.closest('[data-gpu-spec]');
  if (row) { toggleGpu(row.dataset.gpuSpec); return; }
  const removeBtn = e.target.closest('[data-remove-gpu]');
  if (removeBtn) { removeCustomGpu(removeBtn.dataset.removeGpu); return; }
  if (e.target.closest('[data-action="add-custom-gpu"]')) {
    const path  = document.getElementById('gpu-custom-path')?.value?.trim();
    const accel = document.getElementById('gpu-custom-accel')?.value;
    const os    = document.getElementById('gpu-custom-os')?.value;
    if (path && accel && os) addCustomGpu(path, accel, os);
  }
}

async function toggleGpu(spec) {
  const current = new Set(S.state?.gpu_libs || []);
  const action = current.has(spec) ? 'remove' : 'add';
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/gpu`, { action, spec });
    await refreshState();
    refreshSection('gpu');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function addCustomGpu(path, accel, os) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/custom-gpu`, { action: 'add', path, accel, os });
    await refreshState();
    refreshSection('gpu');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function removeCustomGpu(path) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/custom-gpu`, { action: 'remove', path });
    await refreshState();
    refreshSection('gpu');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// Advanced section
// ============================================================================
function renderAdvancedContent() {
  const s = S.state;
  const zipalign = s?.custom_zipalign || '';
  const isCustomArgs = !!(s?.args_file);
  return `<div class="section-desc">${SECTION_DESCS.advanced}</div>
    <div class="config-row">
      <div class="config-label">Custom zipalign</div>
      <input class="config-input" id="adv-zipalign" placeholder="/path/to/zipalign" value="${esc(zipalign)}">
      <button class="btn btn-secondary btn-browse btn-small" onclick="browseFile('adv-zipalign','open','Select zipalign binary')">Browse…</button>
      <button class="btn btn-secondary btn-small" data-action="set-zipalign">Set</button>
      ${zipalign ? `<button class="btn btn-secondary btn-small" data-action="clear-zipalign">Clear</button>` : ''}
    </div>
    <div class="config-row">
      <div class="config-label">.args file</div>
      <span class="args-badge ${isCustomArgs ? 'custom' : 'generated'}">
        ${isCustomArgs ? 'Custom override active' : 'Generated from config'}
      </span>
      <div style="flex:1"></div>
      <button class="btn btn-secondary btn-small" data-action="edit-args">Edit .args…</button>
    </div>
    <div class="inline-note">Override generated args to pass raw CLI flags to the embedded llamafile.</div>`;
}

function handleAdvancedClick(e) {
  if (e.target.closest('[data-action="set-zipalign"]')) {
    const val = document.getElementById('adv-zipalign')?.value?.trim();
    if (val) setCustomZipalign(val);
  } else if (e.target.closest('[data-action="clear-zipalign"]')) {
    setCustomZipalign('');
  } else if (e.target.closest('[data-action="edit-args"]')) {
    showArgsEditor();
  }
}

async function setCustomRuntime(path) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/runtime`, { path });
    await refreshState();
    refreshSection('version');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function setCustomZipalign(path) {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/zipalign`, { path });
    await refreshState();
    refreshSection('advanced');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// .args editor modal
// ============================================================================
async function showArgsEditor() {
  // If opened from inside the Advanced section modal, commit that modal first
  // so we don't leave the section-modal state hanging after the args editor closes.
  if (S.modalSection) {
    S.modalSection = null;
    S.modalSnapshot = null;
  }

  let argsContent = '';
  try { argsContent = await apiGet(`/api/recipes/${S.recipeId}/state/args`); }
  catch (_) {}

  const isCustom = !!(S.state?.args_file);
  const bodyHtml = `
    <span class="args-badge ${isCustom ? 'custom' : 'generated'}">
      ${isCustom ? 'Custom override active' : 'Generated from config'}
    </span>
    <textarea class="args-editor" id="args-editor-ta" spellcheck="false">${esc(argsContent)}</textarea>
    <div class="inline-note">Saving creates a .args override — Config section settings are ignored while active.</div>`;

  showModal('.args editor', bodyHtml, null, {
    customFooter: `
      <button class="btn btn-secondary btn-small btn-danger-hover" onclick="resetArgsOverride()" ${!isCustom ? 'disabled' : ''}>Reset to Generated</button>
      <div style="flex:1"></div>
      <button class="btn btn-secondary" onclick="closeModal()">Close</button>
      <button class="btn btn-primary" onclick="saveArgsOverride()">Save Override</button>`,
  });
}

async function saveArgsOverride() {
  const content = document.getElementById('args-editor-ta')?.value || '';
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/args-content`, { content });
    await refreshState();
    closeModal();
    refreshSection('config');
    refreshSection('advanced');
    showToast('.args override saved', 'ok');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

async function resetArgsOverride() {
  try {
    await apiPost(`/api/recipes/${S.recipeId}/state/args-content`, { content: '' });
    await refreshState();
    closeModal();
    refreshSection('config');
    refreshSection('advanced');
    showToast('.args override cleared', 'ok');
    autoValidate();
  } catch (e) { showToast(e.message, 'err'); }
}

// ============================================================================
// Build (SSE streaming)
// ============================================================================
async function startBuild() {
  if (S.building) { cancelBuild(); return; }
  S.building = true;
  S.buildCompleted = false;
  setBuildingState(true);
  clearLog();
  showProgressPanel(true);
  pushLog({ level: 'info', message: 'Starting build…' });
  startBuildTicker();

  try {
    const resp = await fetch(`/api/recipes/${S.recipeId}/build`, { method: 'POST' });
    if (!resp.ok) {
      const e = await resp.json().catch(() => ({ error: resp.statusText }));
      throw new Error(e.error || resp.statusText);
    }

    const reader = resp.body.getReader();
    const dec = new TextDecoder();
    let buf = '';

    // Labeled outer so we can break from nested for-loops when build ends
    outer: while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      const parts = buf.split('\n\n');
      buf = parts.pop();
      for (const part of parts) {
        for (const line of part.split('\n')) {
          if (!line.startsWith('data: ')) continue;
          try {
            const ev = JSON.parse(line.slice(6));
            handleBuildEvent(ev);
            if (S.buildCompleted) break outer;
          } catch (_) {}
        }
      }
    }

    if (!S.buildCompleted) {
      pushLog({ level: 'warn', message: 'Build stream ended unexpectedly.' });
      setStatus('Interrupted', 'err');
    }
  } catch (e) {
    pushLog({ level: 'error', message: `Error: ${e.message}` });
    setStatus('Build failed', 'err');
  } finally {
    S.building = false;
    stopBuildTicker();
    setBuildingState(false);
    if (S.buildCompleted) {
      // Refresh status after showing result for 4 seconds
      setTimeout(() => { if (!S.building) autoValidate(); }, 4000);
    }
  }
}

function handleBuildEvent(ev) {
  switch (ev.stage) {
    case 'starting':
      pushLog({ level: 'info', message: 'Build started — downloading and assembling components…' });
      setStatus('Building…', 'running');
      setBuildPhase('Preparing');
      showProgressPanel(true);
      break;
    case 'complete': {
      S.buildCompleted = true;
      const size = ev.size ? ` (${fmtBytes(ev.size)})` : '';
      pushLog({ level: 'info', message: `✓ Build complete: ${ev.path}${size}`, cls: 'ok' });
      pushLog({ level: 'info', message: '💾 Do you like this recipe? Click the recipe badge in the top-right and save it.' });
      setStatus('Build complete ✓', 'ok');
      setBuildPhase('Complete');
      markAllDownloadsDone();
      showToast('Build complete: ' + (ev.path || ''), 'ok');
      break;
    }
    case 'error':
      S.buildCompleted = true;
      if (isUserCancellation(ev.message)) {
        // User-initiated cancel — not a failure, just an info-level note.
        pushLog({ level: 'warn', message: 'Build cancelled' });
        setStatus('Build cancelled', 'warn');
        setBuildPhase('Cancelled');
      } else {
        pushLog({ level: 'error', message: `✗ ${ev.message}` });
        setStatus('Build failed', 'err');
        setBuildPhase('Failed');
        showToast('Build failed', 'err');
        showBuildErrorModal(ev.message);
      }
      break;
    case 'log': {
      // Forwarded tracing event. Cache every event regardless of current filter.
      pushLog({
        level: (ev.level || 'info').toLowerCase(),
        message: formatLogLine(ev),
      });
      maybeUpdateProgress(ev);
      maybeUpdatePhase(ev);
      break;
    }
    default:
      break;
  }
}

// Format a tracing log event as a single line, including structured fields.
function formatLogLine(ev) {
  const msg = ev.message || '';
  const fields = ev.fields || {};
  const keys = Object.keys(fields);
  if (!keys.length) return msg;
  const extras = keys.map(k => `${k}=${fields[k]}`).join(' ');
  return msg ? `${msg} ${extras}` : extras;
}

// Heuristic: treat download progress events as anything with bytes_total/bytes_downloaded.
function maybeUpdateProgress(ev) {
  const f = ev.fields || {};
  if (f.bytes_total == null && f.bytes_downloaded == null) return;
  const filename = f.filename || 'download';
  const total = Number(f.bytes_total) || 0;
  const downloaded = Number(f.bytes_downloaded) || 0;
  const pct = total > 0 ? Math.min(100, (downloaded / total) * 100) : 0;
  const done = total > 0 && downloaded >= total;
  S.downloads[filename] = { downloaded, total, pct, done };
  renderProgressBars();
}

// Treat any info-level "preparing/downloading/bundling/running" message as a phase marker.
function maybeUpdatePhase(ev) {
  if ((ev.level || '').toLowerCase() !== 'info') return;
  const msg = ev.message || '';
  if (/^(preparing|downloading|bundling|running|verifying|using|build started)/i.test(msg)) {
    setBuildPhase(msg);
  }
}

const LOG_LEVEL_ORDER = { debug: 0, info: 1, warn: 2, error: 3 };

function startBuildTicker() {
  let tick = 0;
  const frames = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏'];
  S.buildTicker = setInterval(() => {
    if (!S.building) return;
    const el = document.getElementById('build-status');
    if (el?.classList.contains('running')) {
      el.textContent = `${frames[tick % frames.length]} Building…`;
      tick++;
    }
  }, 100);
}

function stopBuildTicker() {
  if (S.buildTicker) { clearInterval(S.buildTicker); S.buildTicker = null; }
}

async function cancelBuild() {
  // Confirmation modal — same UX as the TUI's cancel-build prompt. The user
  // explicitly opts in before we fire /api/cancel.
  showModal(
    'Cancel build?',
    `<p>The current build will stop. Already-downloaded components stay in the cache so the next build can resume.</p>`,
    () => doCancelBuild(),
    {
      customFooter: `
        <button class="btn btn-secondary" onclick="closeModal()">Keep building</button>
        <button class="btn btn-primary" id="modal-ok">Cancel build</button>`,
    }
  );
  const okBtn = document.getElementById('modal-ok');
  if (okBtn) okBtn.onclick = () => { closeModal(); doCancelBuild(); };
}

async function doCancelBuild() {
  try {
    await apiPost('/api/cancel');
    pushLog({ level: 'warn', message: 'Cancellation requested…' });
    setStatus('Cancelling…', 'running');
  } catch (e) { showToast(e.message, 'err'); }
}

// "Cancelled" / "Download cancelled" are user-initiated and should not surface
// as build-failed errors. Mirrors `is_user_cancellation` in the TUI binary.
function isUserCancellation(message) {
  if (!message) return false;
  const m = String(message).trim().toLowerCase();
  return m === 'cancelled' || m === 'download cancelled';
}

// Show the build-failed error in a modal with the raw error text. The same
// message is also appended to the log pane (handled by the caller).
function showBuildErrorModal(message) {
  const safe = (message || 'Build failed').replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const body = `
    <p style="margin-bottom: 0.5em;">The build did not complete. The error was:</p>
    <pre class="modal-error-body" style="white-space: pre-wrap; word-break: break-word;">${safe}</pre>`;
  showModal('Build failed', body, null, {
    customFooter: `<button class="btn btn-primary" onclick="closeModal()">Dismiss</button>`,
  });
}

function setBuildingState(building) {
  const btn = document.getElementById('btn-build');
  if (btn) {
    btn.textContent = building ? '■  Cancel' : '▶\u00a0 Build';
    btn.className = building
      ? 'btn btn-secondary build-btn'
      : 'btn btn-primary build-btn';
  }
}

// ============================================================================
// Reset
// ============================================================================
async function confirmReset() {
  showModal('Reset recipe',
    '<p style="color:var(--text-secondary);font-size:13px">Clear all settings and return to defaults?</p>',
    async () => {
      try {
        await apiPost(`/api/recipes/${S.recipeId}/state/reset`);
        await refreshState();
        S.expandedGroups = {};
        S.modelSearch = '';
        S.modelTag = null;
        renderAll();
        autoValidate();
      } catch (e) { showToast(e.message, 'err'); }
    }
  );
}

// ============================================================================
// Modal
// ============================================================================
function showModal(title, bodyHtml, onOk, opts = {}) {
  document.getElementById('modal-title').textContent = title;
  document.getElementById('modal-body').innerHTML = bodyHtml;

  const modalEl = document.getElementById('modal');
  if (modalEl) modalEl.classList.toggle('modal-section', !!opts.sectionEditor);

  const footer = document.getElementById('modal-footer');
  if (opts.customFooter) {
    footer.innerHTML = opts.customFooter;
  } else {
    footer.innerHTML = `
      <button class="btn btn-secondary" onclick="closeModal()">Cancel</button>
      <button class="btn btn-primary" id="modal-ok">OK</button>`;
    const okBtn = document.getElementById('modal-ok');
    if (onOk) {
      okBtn.style.display = '';
      okBtn.onclick = () => { closeModal(); onOk(); };
    } else {
      okBtn.style.display = 'none';
    }
  }

  document.getElementById('modal-overlay').classList.remove('hidden');
}

function closeModal() {
  document.getElementById('modal-overlay').classList.add('hidden');
}

document.getElementById('modal-overlay')?.addEventListener('click', function(e) {
  if (e.target !== this) return;
  // Section-editor modals require explicit OK/Cancel — ignore backdrop click.
  if (S.modalSection) return;
  closeModal();
});

// ============================================================================
// Log pane — caches every event in S.allLogs; filter is client-side only.
// ============================================================================

// Push a log event into the cache and render it if it passes the current filter.
function pushLog(entry) {
  const level = (entry.level || 'info').toLowerCase();
  S.allLogs.push({ level, message: entry.message || '', cls: entry.cls });
  // Cap cache so memory doesn't blow up on very long builds.
  if (S.allLogs.length > 5000) S.allLogs.splice(0, S.allLogs.length - 5000);

  const pane = document.getElementById('log-pane');
  if (!pane) return;
  if (!shouldShowLevel(level)) return;
  appendLogLine(pane, level, entry.message || '', entry.cls);
}

// Legacy free-text logger — routed through the cache so clearing still works.
function addLog(msg, cls) {
  pushLog({ level: 'info', message: msg, cls });
}

function appendLogLine(pane, level, message, overrideCls) {
  const empty = pane.querySelector('.log-empty');
  if (empty) empty.remove();
  const cls = overrideCls || (
    level === 'error' ? 'err' :
    level === 'warn'  ? 'warn' :
    level === 'debug' ? 'dim' :
    level === 'trace' ? 'dim' : 'info'
  );
  const line = document.createElement('div');
  line.className = `log-line ${cls}`;
  line.textContent = `[${level.toUpperCase().padEnd(5)}] ${message}`;
  pane.appendChild(line);
  pane.scrollTop = pane.scrollHeight;
}

function shouldShowLevel(level) {
  const lv = LOG_LEVEL_ORDER[level] ?? 1;
  const fl = LOG_LEVEL_ORDER[S.logLevel] ?? 1;
  return lv >= fl;
}

// Re-render the log pane from the cache (called when the filter changes).
function renderLogPane() {
  const pane = document.getElementById('log-pane');
  if (!pane) return;
  pane.innerHTML = '';
  let any = false;
  for (const e of S.allLogs) {
    if (!shouldShowLevel(e.level)) continue;
    appendLogLine(pane, e.level, e.message, e.cls);
    any = true;
  }
  if (!any) pane.innerHTML = '<div class="log-empty">No build output yet.</div>';
}

function clearLog() {
  S.allLogs = [];
  S.downloads = {};
  S.buildPhase = '';
  renderProgressBars();
  setBuildPhase('');
  const pane = document.getElementById('log-pane');
  if (pane) pane.innerHTML = '<div class="log-empty">No build output yet.</div>';
}

function setStatus(msg, cls) {
  const el = document.getElementById('build-status');
  if (!el) return;
  el.textContent = msg;
  el.className = `build-status${cls ? ' ' + cls : ''}`;
  // Clickable iff it's reporting validation errors/warnings we have details for.
  const hasDetails = (cls === 'err' || cls === 'warn') && (S.validationMessages || []).length;
  if (hasDetails) {
    el.classList.add('clickable');
    el.onclick = showValidationDetails;
    el.title = 'Click to view details';
  } else {
    el.classList.remove('clickable');
    el.onclick = null;
    el.title = '';
  }
}

// ============================================================================
// Toast notifications
// ============================================================================
function showToast(msg, type) {
  const container = document.getElementById('toasts');
  if (!container) return;
  const el = document.createElement('div');
  el.className = `toast ${type || ''}`;
  el.textContent = msg;
  container.appendChild(el);
  setTimeout(() => el.classList.add('fading'), 2500);
  setTimeout(() => el.remove(), 2800);
}

// ============================================================================
// Log level filter
// ============================================================================
function setLogLevel(level) {
  S.logLevel = level;
  // Update button active states.
  document.querySelectorAll('.log-level-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.level === level);
  });
  // Re-render the pane from the cached logs so the filter applies retroactively.
  renderLogPane();
}

// ============================================================================
// Progress panel — build phase + per-file download bars
// ============================================================================
function showProgressPanel(visible) {
  const el = document.getElementById('progress-section');
  if (el) el.style.display = visible ? '' : 'none';
}

function setBuildPhase(phase) {
  S.buildPhase = phase || '';
  const el = document.getElementById('progress-phase');
  if (el) el.textContent = S.buildPhase;
}

function renderProgressBars() {
  const el = document.getElementById('progress-bars');
  if (!el) return;
  const names = Object.keys(S.downloads);
  if (!names.length) { el.innerHTML = ''; return; }

  let html = '';
  for (const name of names) {
    const d = S.downloads[name];
    const pct = Math.max(0, Math.min(100, d.pct || 0));
    const total = d.total ? fmtBytes(d.total) : '—';
    const downloaded = fmtBytes(d.downloaded);
    const stats = d.done ? `${total}  done` : `${downloaded} / ${total}  (${pct.toFixed(1)}%)`;
    html += `
      <div class="progress-item ${d.done ? 'done' : ''}">
        <div class="progress-item-label" title="${esc(name)}">${esc(name)}</div>
        <div class="progress-item-stats">${stats}</div>
        <div class="progress-item-bar">
          <div class="progress-item-fill" style="width:${pct}%"></div>
        </div>
      </div>`;
  }
  el.innerHTML = html;
}

function markAllDownloadsDone() {
  for (const k of Object.keys(S.downloads)) {
    const d = S.downloads[k];
    if (d.total && d.downloaded < d.total) d.downloaded = d.total;
    d.pct = 100;
    d.done = true;
  }
  renderProgressBars();
}

// ============================================================================
// Cancel on tab close — tells the backend to stop downloads/build.
// Downloads resume from the cache on next start.
// ============================================================================
window.addEventListener('beforeunload', () => {
  if (!S.building) return;
  // Fire-and-forget; `keepalive` lets the request survive page teardown.
  try {
    fetch('/api/cancel', { method: 'POST', keepalive: true });
  } catch (_) {}
});

// ============================================================================
// Entry point
// ============================================================================
document.addEventListener('DOMContentLoaded', init);
