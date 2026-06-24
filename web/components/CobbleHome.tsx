import {
  ArrowRight,
  BookOpen,
  Braces,
  Code2,
  Download,
  FileCode2,
  ShieldCheck,
  Terminal
} from "lucide-react";
import Link from "next/link";
import { GitHubMark } from "./BrandIcons";
import { CodeBlock } from "./SyntaxHighlighter";

const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

const examples = [
  {
    title: "Load Events",
    code: `import stdlib
from stdlib import event

def on_load():
    text.tellraw("@a", text.colored("Ready", "green"))

stdlib.addEventListener(event.LOAD, on_load)`
  },
  {
    title: "Modern Items",
    code: `def reward(player):
    /give {player} minecraft:diamond_sword[
        minecraft:custom_name='{"text":"Cobble Blade"}'
    ] 1`
  },
  {
    title: "Data Pack Resources",
    code: `datapack.function_tag("minecraft:load", ["demo:on_load"])
datapack.predicate("checks/always", {
    "condition": "minecraft:random_chance",
    "chance": 1
})`
  }
];

export function CobbleHome() {
  return (
    <main>
      <section className="home-hero">
        <img className="hero-image" src={`${basePath}/cobble-workshop.jpg`} alt="" />
        <div className="hero-scrim" />

        <nav className="topbar" aria-label="Primary">
          <Link className="brand" href="/">
            <span className="brand-mark">C</span>
            <span>Cobble</span>
          </Link>
          <div className="nav-actions">
            <Link className="nav-link" href="/try">
              <Terminal size={16} />
              <span>Try</span>
            </Link>
            <a className="nav-link" href="https://github.com/deveworld/cobble/tree/main/docs">
              <BookOpen size={16} />
              <span>Docs</span>
            </a>
            <a className="nav-link" href="https://github.com/deveworld/cobble">
              <GitHubMark width={16} height={16} />
              <span>GitHub</span>
            </a>
          </div>
        </nav>

        <div className="home-hero-copy">
          <p className="eyebrow">Minecraft Java Edition 26.1.2 · Pack Format 101.1</p>
          <h1>Cobble</h1>
          <p>
            A modern, Python-like language for creating Minecraft data packs with
            functions, events, resources, validation, and generated metadata.
          </p>
          <p className="release-install">
            0.7.3 stable · Minecraft Java Edition 26.1.2
          </p>
          <div className="hero-actions">
            <Link className="command-button hero-command" href="/try">
              <Terminal size={17} />
              <span>Open compiler</span>
              <ArrowRight size={17} />
            </Link>
            <a className="secondary-button" href="https://github.com/deveworld/cobble">
              <Code2 size={17} />
              <span>Source</span>
            </a>
          </div>
        </div>
      </section>

      <section className="home-summary-band" aria-label="Cobble output summary">
        <div className="home-summary">
          <FileCode2 size={20} />
          <strong>Readable source</strong>
          <span>Indentation, functions, imports, and thin helper APIs.</span>
        </div>
        <div className="home-summary">
          <Braces size={20} />
          <strong>Real data packs</strong>
          <span>Functions, pack metadata, tags, predicates, dialogs, and JSON.</span>
        </div>
        <div className="home-summary">
          <ShieldCheck size={20} />
          <strong>Validation path</strong>
          <span>Generated commands can be checked against Minecraft's command tree.</span>
        </div>
        <div className="home-summary">
          <Download size={20} />
          <strong>Browser export</strong>
          <span>Download generated files or a data pack ZIP from `/try`.</span>
        </div>
      </section>

      <section className="examples-band" aria-label="Cobble examples">
        <div className="section-heading">
          <p className="eyebrow">Examples</p>
          <h2>Write data pack logic directly</h2>
          <p>
            Cobble stays close to Minecraft output while removing repetitive
            function and resource boilerplate.
          </p>
        </div>
        <div className="example-grid">
          {examples.map((example) => (
            <article className="example-card" key={example.title}>
              <div className="example-title">
                <Code2 size={16} />
                <strong>{example.title}</strong>
              </div>
              <CodeBlock code={example.code} language="cobble" />
            </article>
          ))}
        </div>
      </section>

      <section className="compiler-band" aria-label="Compiler route">
        <div className="compiler-copy">
          <p className="eyebrow">In Browser</p>
          <h2>Try the compiler when you are ready to inspect output.</h2>
          <p>
            The Rust parser and transpiler run through WebAssembly. The compiler
            view shows functions, resources, metadata, diagnostics, and ZIP output.
          </p>
        </div>
        <Link className="command-button hero-command" href="/try">
          <Terminal size={17} />
          <span>Go to /try</span>
          <ArrowRight size={17} />
        </Link>
      </section>
    </main>
  );
}
