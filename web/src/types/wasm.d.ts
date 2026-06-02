declare module "@/src/wasm/pkg/cobble_web_wasm.js" {
  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function init(
    moduleOrPath?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>
  ): Promise<unknown>;

  export function compile_cobble(
    source: string,
    namespace?: string,
    description?: string
  ): unknown;
}
