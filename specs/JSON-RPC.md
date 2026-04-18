# JSON-RPC Interactive Mode

The builder exposes a JSON-RPC 2.0 interface over stdin/stdout via `--interactive`. This is the universal protocol for driving the builder programmatically -- the HTTP server (Phase 6) and any future GUI will use the same method names and semantics.

```bash
llamafile-builder --interactive
```

One JSON object per line on stdin. One response per line on stdout. Tracing output goes to stderr (control with `RUST_LOG`).


## Protocol

Requests:
```json
{"jsonrpc":"2.0","id":1,"method":"method_name","params":{...}}
```

Responses:
```json
{"jsonrpc":"2.0","id":1,"result":{...}}
```

Errors:
```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"description"}}
```

Notifications (no `id`, pushed during build):
```json
{"jsonrpc":"2.0","method":"progress","params":{"stage":"starting"}}
```

### Error codes

| Code | Meaning |
|------|---------|
| -32700 | Parse error (malformed JSON) |
| -32601 | Method not found |
| -32602 | Invalid params (missing required field) |
| -32000 | Application error (model not found, build failed, etc.) |


## Methods

### Queries (read-only)

#### list_versions
```json
{"jsonrpc":"2.0","id":1,"method":"list_versions"}
```
Returns the full versions array from config.

#### list_models
```json
{"jsonrpc":"2.0","id":1,"method":"list_models"}
{"jsonrpc":"2.0","id":1,"method":"list_models","params":{"search":"qwen"}}
{"jsonrpc":"2.0","id":1,"method":"list_models","params":{"tag":"vision"}}
{"jsonrpc":"2.0","id":1,"method":"list_models","params":{"search":"qwen","tag":"chat"}}
```
Returns filtered models array. Both `search` and `tag` are optional.

#### show_model
```json
{"jsonrpc":"2.0","id":1,"method":"show_model","params":{"id":"Qwen/Qwen3-0.6B-GGUF"}}
```
Returns full model detail including files and quantizations.

#### system_info
```json
{"jsonrpc":"2.0","id":1,"method":"system_info"}
```
Returns `{ os, arch, cpu_cores, memory_gb, has_nvidia_gpu, has_amd_gpu, has_metal }`.

#### config_schema
```json
{"jsonrpc":"2.0","id":1,"method":"config_schema"}
```
Returns the tagged `ConfigSchemaItem` array that drives the configuration UI (see [LIBRARY.md](LIBRARY.md#configuration-schema)). Useful for clients that render dynamic forms from the schema.

#### list_ollama_models
```json
{"jsonrpc":"2.0","id":1,"method":"list_ollama_models"}
```
Returns array of local Ollama models.

#### list_lmstudio_models
```json
{"jsonrpc":"2.0","id":1,"method":"list_lmstudio_models"}
```
Returns array of local LM Studio models.


### Recipe management

Recipes are named, UUID-identified build configurations that persist across sessions. All state mutations operate on the "current" recipe.

#### create_recipe
```json
{"jsonrpc":"2.0","id":1,"method":"create_recipe"}
```
Creates a new empty recipe, sets it as current. Returns `{ id }`.

#### list_recipes
```json
{"jsonrpc":"2.0","id":1,"method":"list_recipes"}
```
Returns array of `{ id, name, model, updated_at, current }`.

#### load_recipe
```json
{"jsonrpc":"2.0","id":1,"method":"load_recipe","params":{"id":"name-or-uuid"}}
```
Loads a recipe as the current session config. Returns `{ id, name }`.

#### save_recipe
```json
{"jsonrpc":"2.0","id":1,"method":"save_recipe","params":{"name":"my-recipe"}}
```
Names the current recipe and resolves all defaults for a full snapshot. Returns `{ ok, id, name }`.

#### delete_recipe
```json
{"jsonrpc":"2.0","id":1,"method":"delete_recipe","params":{"id":"name-or-uuid"}}
```
Deletes a recipe by name or UUID.


### State mutations

Each mutation operates on the current recipe (created via `create_recipe` or loaded via `load_recipe`). A recipe is auto-created if none exists. Every mutation auto-persists via ConfigStore and returns `{ ok, recipe_id }`.

#### set_version
```json
{"jsonrpc":"2.0","id":1,"method":"set_version","params":{"group":"llamafile 0.10","version":"0.10.0"}}
```

#### set_model
```json
{"jsonrpc":"2.0","id":1,"method":"set_model","params":{"id":"Qwen/Qwen3-0.6B-GGUF"}}
```
Returns error if model ID not found in registry. Clears other model sources.

#### set_model_file
```json
{"jsonrpc":"2.0","id":1,"method":"set_model_file","params":{"path":"/path/to/model.gguf"}}
```

#### set_ollama_model
```json
{"jsonrpc":"2.0","id":1,"method":"set_ollama_model","params":{"name":"qwen3:0.6b"}}
```
Scans local Ollama models, selects by display name. Returns error if not found locally.

#### set_lmstudio_model
```json
{"jsonrpc":"2.0","id":1,"method":"set_lmstudio_model","params":{"name":"Qwen3.5-0.8B-GGUF"}}
```
Scans local LM Studio models, selects by directory name or display name. Returns error if not found locally.

#### set_quantization
```json
{"jsonrpc":"2.0","id":1,"method":"set_quantization","params":{"role":"weights","name":"Q8_0"}}
{"jsonrpc":"2.0","id":1,"method":"set_quantization","params":{"role":"mmproj","name":"Q8_0"}}
```

#### set_config
```json
{"jsonrpc":"2.0","id":1,"method":"set_config","params":{"key":"server_enabled","value":false}}
{"jsonrpc":"2.0","id":1,"method":"set_config","params":{"key":"context_length","value":8192}}
```
Value can be any JSON type (bool, number, string).

#### set_output
```json
{"jsonrpc":"2.0","id":1,"method":"set_output","params":{"path":"/tmp/my.llamafile"}}
```

#### set_args_file
```json
{"jsonrpc":"2.0","id":1,"method":"set_args_file","params":{"path":"/path/to/custom.args"}}
```

#### add_gpu / remove_gpu
```json
{"jsonrpc":"2.0","id":1,"method":"add_gpu","params":{"spec":"cuda"}}
{"jsonrpc":"2.0","id":1,"method":"add_gpu","params":{"spec":"cuda:linux"}}
{"jsonrpc":"2.0","id":1,"method":"remove_gpu","params":{"spec":"cuda"}}
```

#### set_runtime
```json
{"jsonrpc":"2.0","id":1,"method":"set_runtime","params":{"path":"/path/to/llamafile"}}
```

#### set_zipalign
```json
{"jsonrpc":"2.0","id":1,"method":"set_zipalign","params":{"path":"/path/to/zipalign"}}
```

#### reset
```json
{"jsonrpc":"2.0","id":1,"method":"reset"}
```
Clears all session state back to empty (defaults will be applied on next query/build).


### State queries

#### get_state
```json
{"jsonrpc":"2.0","id":1,"method":"get_state"}
```
Returns the raw `SavedState` -- what the user has explicitly configured (not including defaults).

#### show_args
```json
{"jsonrpc":"2.0","id":1,"method":"show_args"}
```
Returns the .args file content that would be generated. Applies defaults for anything not configured.

#### validate
```json
{"jsonrpc":"2.0","id":1,"method":"validate"}
```
Returns array of `{ level: "warning"|"error", message: "..." }`. Empty array means ready to build.

#### resolved_components
```json
{"jsonrpc":"2.0","id":1,"method":"resolved_components"}
```
Returns array of `{ role, source, zip_path }` -- what `build` would assemble.


### Build

#### build
```json
{"jsonrpc":"2.0","id":1,"method":"build"}
```

Triggers the build pipeline. During the build, progress notifications are pushed:
```json
{"jsonrpc":"2.0","method":"progress","params":{"stage":"starting"}}
```

On success:
```json
{"jsonrpc":"2.0","id":1,"result":{"output_path":"/tmp/my.llamafile","size":670394809}}
```

On failure:
```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Build failed: ..."}}
```


## Test examples

These can be piped directly to the builder for automated testing.

### Simple build (all defaults)

```bash
cat << 'EOF' | llamafile-builder --interactive 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"build"}
EOF
```
Builds the default model (Qwen3 0.6B) with the default version (0.10.0).

### Configure and build

```bash
cat << 'EOF' | llamafile-builder --interactive 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"set_model","params":{"id":"Qwen/Qwen3-0.6B-GGUF"}}
{"jsonrpc":"2.0","id":2,"method":"set_config","params":{"key":"server_enabled","value":false}}
{"jsonrpc":"2.0","id":3,"method":"set_output","params":{"path":"/tmp/test.llamafile"}}
{"jsonrpc":"2.0","id":4,"method":"show_args"}
{"jsonrpc":"2.0","id":5,"method":"build"}
EOF
```

### Multimodal with quantization selection

```bash
cat << 'EOF' | llamafile-builder --interactive 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"set_model","params":{"id":"Qwen/Qwen3-VL-4B-Instruct-GGUF"}}
{"jsonrpc":"2.0","id":2,"method":"set_quantization","params":{"role":"weights","name":"Q8_0"}}
{"jsonrpc":"2.0","id":3,"method":"set_quantization","params":{"role":"mmproj","name":"Q8_0"}}
{"jsonrpc":"2.0","id":4,"method":"resolved_components"}
{"jsonrpc":"2.0","id":5,"method":"validate"}
{"jsonrpc":"2.0","id":6,"method":"set_output","params":{"path":"/tmp/qwen3vl-q8.llamafile"}}
{"jsonrpc":"2.0","id":7,"method":"build"}
EOF
```

### Inspect without building

```bash
cat << 'EOF' | llamafile-builder --interactive 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"list_models","params":{"tag":"vision"}}
{"jsonrpc":"2.0","id":2,"method":"show_model","params":{"id":"mistralai/Ministral-3-3B-Instruct-2512-GGUF"}}
{"jsonrpc":"2.0","id":3,"method":"set_model","params":{"id":"mistralai/Ministral-3-3B-Instruct-2512-GGUF"}}
{"jsonrpc":"2.0","id":4,"method":"show_args"}
{"jsonrpc":"2.0","id":5,"method":"resolved_components"}
{"jsonrpc":"2.0","id":6,"method":"validate"}
EOF
```

### Error handling

```bash
cat << 'EOF' | llamafile-builder --interactive 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"set_model","params":{"id":"nonexistent/model"}}
{"jsonrpc":"2.0","id":2,"method":"bogus_method"}
{"jsonrpc":"2.0","id":3,"method":"set_quantization","params":{"role":"weights"}}
not valid json
EOF
```

Expected responses:
```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"model not found: nonexistent/model"}}
{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"method not found: bogus_method"}}
{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"missing 'name' param"}}
{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error: ..."}}
```
