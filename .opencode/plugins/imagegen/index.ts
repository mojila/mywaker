import { Plugin } from "@opencode/plugin"
import { promises as fs } from "node:fs"
import path from "node:path"

const DEFAULT_MODEL = "agnes-image-2-5-flash:free"
const ENDPOINT = "https://kenari.id/v1/images/generations"
const DEFAULT_OUTPUT = "assets"

function slugify(value: string): string {
  const slug = value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
  return slug || "image"
}

function timestamp(): string {
  const now = new Date()
  const pad = (n: number) => String(n).padStart(2, "0")
  return (
    `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-` +
    `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`
  )
}

interface GenerateInput {
  prompt: string
  n?: number
  size?: string
  background?: "auto" | "transparent" | "opaque"
  output?: string
  filename?: string
}

export default Plugin.define({
  id: "imagegen",
  async setup(ctx) {
    const baseDir = ctx.location.directory
    const options = (ctx.options ?? {}) as Record<string, unknown>
    const model = (options.model as string) ?? process.env.KENARI_IMAGE_MODEL ?? DEFAULT_MODEL

    await ctx.tool.transform((editor) => {
      editor.add({
        name: "generate_image",
        description:
          "Generate an image asset from a text prompt using the kenari `" +
          model +
          "` image model and save it to disk as PNG. " +
          "Returns the written file path(s) so the image can be referenced as an asset.",
        input: {
          type: "object",
          properties: {
            prompt: {
              type: "string",
              description: "Description of the image to generate.",
            },
            n: {
              type: "integer",
              minimum: 1,
              maximum: 4,
              description: "Number of images to generate. Defaults to 1.",
            },
            size: {
              type: "string",
              description:
                "Image size hint, e.g. '1024x1024', '1536x1024', '1024x1536'. Defaults to '1024x1024'. Not a guarantee.",
            },
            background: {
              type: "string",
              enum: ["auto", "transparent", "opaque"],
              description: "Background mode. Defaults to 'auto'.",
            },
            output: {
              type: "string",
              description:
                "Directory to save the asset(s) into, relative to the project root. Defaults to 'assets'.",
            },
            filename: {
              type: "string",
              description:
                "Base filename without extension. Defaults to a slug of the prompt plus a timestamp.",
            },
          },
          required: ["prompt"],
          additionalProperties: false,
        },
        execute: async (rawInput, context) => {
          const input = rawInput as GenerateInput
          const n = Math.min(Math.max(input.n ?? 1, 1), 4)
          const size = input.size ?? "1024x1024"
          const background = input.background ?? "auto"
          const output = input.output ?? DEFAULT_OUTPUT

          const apiKey = process.env.KENARI_API_KEY ?? process.env.KN_API_KEY
          if (!apiKey) {
            return {
              title: "generate_image",
              content:
                "No kenari API key found. Set the `KENARI_API_KEY` environment variable to a `kn-...` key " +
                "(https://kenari.id dashboard -> API Keys) and restart OpenCode.",
            }
          }

          await context.progress?.({ status: `Generating ${n} image(s) with ${model}…` })

          let response: Response
          try {
            response = await fetch(ENDPOINT, {
              method: "POST",
              headers: {
                Authorization: `Bearer ${apiKey}`,
                "Content-Type": "application/json",
              },
              body: JSON.stringify({
                model,
                prompt: input.prompt,
                n,
                size,
                background,
                response_format: "b64_json",
              }),
              signal: context.signal,
            })
          } catch (err) {
            const message = err instanceof Error ? err.message : String(err)
            return { title: "generate_image", content: `Kenari request failed: ${message}` }
          }

          let body: any
          try {
            body = await response.json()
          } catch {
            body = undefined
          }

          if (!response.ok) {
            const detail = body?.error?.message ?? (await response.text().catch(() => ""))
            return {
              title: "generate_image",
              content: `Kenari image generation failed (HTTP ${response.status}): ${detail}`,
            }
          }

          const data: any[] = body?.data ?? []
          if (!Array.isArray(data) || data.length === 0) {
            return { title: "generate_image", content: "Kenari returned no images." }
          }

          const base = input.filename
            ? path.parse(input.filename).name
            : `${slugify(input.prompt)}-${timestamp()}`

          const results: Array<{ path: string; rel: string; bytes: number }> = []

          for (let i = 0; i < data.length; i++) {
            const item = data[i]
            let bytes: Buffer
            if (item.b64_json) {
              bytes = Buffer.from(item.b64_json, "base64")
            } else if (item.url) {
              await context.progress?.({ status: `Downloading image ${i + 1}/${data.length}…` })
              const imageResponse = await fetch(item.url, { signal: context.signal })
              if (!imageResponse.ok) {
                return {
                  title: "generate_image",
                  content: `Failed to download image from kenari (HTTP ${imageResponse.status}).`,
                }
              }
              bytes = Buffer.from(await imageResponse.arrayBuffer())
            } else {
              return {
                title: "generate_image",
                content: "Kenari returned an image entry with neither b64_json nor url.",
              }
            }

            const suffix = data.length > 1 ? `-${i + 1}` : ""
            const absolute = path.resolve(baseDir, output, `${base}${suffix}.png`)
            await fs.mkdir(path.dirname(absolute), { recursive: true })
            await fs.writeFile(absolute, bytes)

            results.push({
              path: absolute,
              rel: path.relative(baseDir, absolute),
              bytes: bytes.length,
            })
          }

          const lines = [
            `Generated ${results.length} image asset(s) with ${model} (${size}, background=${background}):`,
            "",
            ...results.map(
              (r, i) => `${i + 1}. ${r.rel} (${r.bytes} bytes) — ${r.path}`,
            ),
          ]

          const revised = data.map((d) => d.revised_prompt).filter(Boolean)
          if (revised.length > 0) {
            lines.push("", "Revised prompt(s):", ...revised.map((r) => `- ${r}`))
          }

          return { title: "generate_image", content: lines.join("\n") }
        },
      })
    })
  },
})