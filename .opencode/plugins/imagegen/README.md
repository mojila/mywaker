# imagegen

OpenCode plugin that generates image assets from the kenari image model
`agnes-image-2-5-flash:free` and writes them to disk as PNG.

Loaded automatically from `.opencode/plugins/imagegen/`.

## Prerequisites

Set a kenari API key (starts with `kn-`) in the environment before starting
OpenCode:

```bash
export KENARI_API_KEY="kn-..."
```

The key is created in the kenari dashboard under **API Keys**, and one key works
for every model. `KN_API_KEY` also works as a fallback.

## Tool

The plugin registers a single tool, **`generate_image`**.

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `prompt` | string | — (required) | Description of the image to generate. |
| `n` | integer | `1` | Number of images (1–4). |
| `size` | string | `1024x1024` | Size hint, e.g. `1536x1024`, `1024x1536`. |
| `background` | enum | `auto` | `auto`, `transparent`, or `opaque`. Note: `agnes-image-2-5-flash:free` rejects `transparent` with a `400`. |
| `output` | string | `assets` | Output directory, relative to the project root. |
| `filename` | string | `<slug>-<timestamp>` | Base filename without extension. |

Generated files are written to `assets/` by default (e.g.
`assets/icon-20261016-123456.png`) and the tool reports both the relative and
absolute path of each asset.

## Configuration

The model and output defaults are resolved in this order:

- Model: plugin option `model` → `KENARI_IMAGE_MODEL` env var →
  `agnes-image-2-5-flash:free`.
- API key: `KENARI_API_KEY` → `KN_API_KEY`.

To override the model via plugin options, load it explicitly in `opencode.jsonc`:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "plugins": [
    {
      "package": "./.opencode/plugins/imagegen",
      "options": { "model": "agnes-image-2-5-flash:free" }
    }
  ]
}
```