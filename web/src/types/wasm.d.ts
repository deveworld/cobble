declare module "@/src/wasm/pkg/cobble_web_wasm.js" {
  export function compile_cobble(
    source: string,
    namespace?: string,
    description?: string
  ): unknown;
}
