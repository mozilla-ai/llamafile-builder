// llamafile Builder - Event Delegation System
// Handles all user interactions via data-action attributes
// Required for Snap/WebKitGTK compatibility (inline handlers are stripped by CSP)

/**
 * Extract data attributes as parameters
 * Converts data-kebab-case to camelCase
 */
export function extractParams(element) {
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
 * Create the global click handler
 * @param {Object} actions - Map of action names to handler functions
 */
export function createClickHandler(actions) {
  return async function handleGlobalClick(event) {
    const target = event.target.closest('[data-action]');
    if (!target) return;

    const action = target.getAttribute('data-action');
    const stopProp = target.getAttribute('data-stop-propagation') === 'true';

    if (stopProp) {
      event.stopPropagation();
    }

    event.preventDefault();

    const params = extractParams(target);
    const handler = actions[action];

    if (handler) {
      try {
        await handler(params, event);
      } catch (err) {
        console.error('Error handling action:', action, err);
      }
    } else {
      console.warn('Unknown action:', action, params);
    }
  };
}

/**
 * Create the global input handler
 * @param {Object} actions - Map of action names to handler functions
 */
export function createInputHandler(actions) {
  return function handleGlobalInput(event) {
    const target = event.target.closest('[data-action]');
    if (!target) return;

    const action = target.getAttribute('data-action');
    const params = extractParams(target);
    const handler = actions[action];

    if (handler) {
      try {
        handler(params, event);
      } catch (err) {
        console.error('Error handling input:', action, err);
      }
    }
  };
}

/**
 * Create the global change handler
 * @param {Object} actions - Map of action names to handler functions
 */
export function createChangeHandler(actions) {
  return async function handleGlobalChange(event) {
    const target = event.target.closest('[data-action]');
    if (!target) return;

    const action = target.getAttribute('data-action');
    const params = extractParams(target);
    const handler = actions[action];

    if (handler) {
      try {
        await handler(params, event);
      } catch (err) {
        console.error('Error handling change:', action, err);
      }
    }
  };
}

/**
 * Create the global keydown handler
 * @param {Object} actions - Map of action names to handler functions
 */
export function createKeydownHandler(actions) {
  return function handleGlobalKeydown(event) {
    if (event.key !== 'Enter') return;

    const target = event.target.closest('[data-action]');
    if (!target) return;

    const action = target.getAttribute('data-action');
    const handler = actions[action];

    if (handler) {
      event.preventDefault();
      try {
        handler({}, event);
      } catch (err) {
        console.error('Error handling keydown:', action, err);
      }
    }
  };
}

/**
 * Attach all event delegation handlers to the document
 * @param {Object} clickActions - Map of click action names to handlers
 * @param {Object} inputActions - Map of input action names to handlers
 * @param {Object} changeActions - Map of change action names to handlers
 * @param {Object} keydownActions - Map of keydown action names to handlers
 */
export function attachEventDelegation(clickActions, inputActions, changeActions, keydownActions) {
  document.addEventListener('click', createClickHandler(clickActions), true);
  document.addEventListener('input', createInputHandler(inputActions), false);
  document.addEventListener('change', createChangeHandler(changeActions), false);
  document.addEventListener('keydown', createKeydownHandler(keydownActions), false);
}
