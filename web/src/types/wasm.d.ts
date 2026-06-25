declare module "@/src/wasm/pkg/cobble_web_wasm.js" {
  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export type CompileFile = {
    path: string;
    kind: string;
    content: string;
  };

  export type CompileSummary = {
    namespace: string;
    pack_format: string;
    minecraft_version: string;
    function_count: number;
    command_count: number;
    resource_count: number;
    file_count: number;
  };

  export type CompileDiagnostic = {
    file: string;
    line: number;
    column: number;
    severity: string;
    kind: string;
    message: string;
    help?: string | null;
    formatted: string;
  };

  export type CompileResponse = {
    ok: boolean;
    files: CompileFile[];
    diagnostics: string[];
    diagnostic_details: CompileDiagnostic[];
    summary: CompileSummary;
    experimental_python_compat?: {
      enabled: boolean;
      mode: string;
      supported_constructs: string[];
      unsupported_detected: CompileDiagnostic[];
    };
  };

  export default function init(
    moduleOrPath?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>
  ): Promise<unknown>;

  export function compile_cobble(
    source: string,
    namespace?: string | null,
    description?: string | null,
    experimentalResourcePack?: boolean | null,
    experimentalPythonCompat?: boolean | null
  ): CompileResponse;
}
