import esbuild from 'esbuild';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isProduction = process.env.NODE_ENV === 'production';

// Copy assets (CSS) to dist
const copyAssetsPlugin = {
  name: 'copy-assets',
  setup(build) {
    build.onStart(() => {
      console.log('📦 Copying assets...');
      const srcDir = path.join(__dirname, 'assets');
      const distDir = path.join(__dirname, 'dist');
      
      if (!fs.existsSync(srcDir)) {
        console.log('⚠️  No source assets directory found');
        return;
      }
      
      if (!fs.existsSync(distDir)) fs.mkdirSync(distDir, { recursive: true });
      
      const assetsDir = path.join(distDir, 'assets');
      if (!fs.existsSync(assetsDir)) fs.mkdirSync(assetsDir, { recursive: true });
      
      fs.readdirSync(srcDir).forEach(file => {
        if (file.endsWith('.css')) {
          fs.copyFileSync(
            path.join(srcDir, file),
            path.join(assetsDir, file)
          );
          console.log(`✓ Copied ${file}`);
        }
      });
    });
  }
};

// Copy index.html to dist
const copyHtmlPlugin = {
  name: 'copy-html',
  setup(build) {
    build.onStart(() => {
      const srcFile = path.join(__dirname, 'index.html');
      const destFile = path.join(__dirname, 'dist', 'index.html');
      
      if (!fs.existsSync(srcFile)) {
        console.log('⚠️  index.html not found');
        return;
      }
      
      const distDir = path.join(__dirname, 'dist');
      if (!fs.existsSync(distDir)) fs.mkdirSync(distDir, { recursive: true });
      
      fs.copyFileSync(srcFile, destFile);
      console.log('✓ Copied index.html');
    });
  }
};

// Patch openvm-verifier to load WASM dynamically (for browser fetch)
const patchOpenVMVerifierPlugin = {
  name: 'patch-openvm-verifier',
  setup(build) {
    build.onStart(() => {
      const verifierDir = path.join(__dirname, 'dist', 'openvm-verifier');
      const jsFile = path.join(verifierDir, 'openvm_wasm_verifier.js');
      
      if (!fs.existsSync(jsFile)) {
        console.log('⚠️  openvm-verifier not found, skipping patch');
        return;
      }
      
      const wrapper = `// Web-compatible wrapper (dynamic WASM loading)
import { __wbg_set_wasm, __wbindgen_init_externref_table } from "./openvm_wasm_verifier_bg.js";
export * from "./openvm_wasm_verifier_bg.js";

let _initialized = false;

export default async function init(input) {
    if (_initialized) return;

    const url = input ?? new URL("openvm_wasm_verifier_bg.wasm", import.meta.url);
    const response = await fetch(url);
    const bytes = await response.arrayBuffer();

    const imports = {};
    imports["./openvm_wasm_verifier_bg.js"] = await import("./openvm_wasm_verifier_bg.js");

    const { instance } = await WebAssembly.instantiate(bytes, imports);
    __wbg_set_wasm(instance.exports);
    if (typeof __wbindgen_init_externref_table === "function")
        __wbindgen_init_externref_table();

    _initialized = true;
}
`;
      
      fs.writeFileSync(jsFile, wrapper);
      console.log('✓ Patched openvm-verifier for dynamic WASM loading');
    });
  }
};

esbuild.build({
  entryPoints: ['assets/index.js'],
  bundle: true,
  outdir: 'dist',
  outbase: '.',
  platform: 'browser',
  target: ['esnext'],
  format: 'esm',
  sourcemap: !isProduction,
  minify: isProduction,
  define: {
    'BACKEND_URL_CONFIG': JSON.stringify(process.env.BACKEND_URL || '/'),
  },
  plugins: [copyAssetsPlugin, copyHtmlPlugin, patchOpenVMVerifierPlugin],
  external: [
    '../uplc/*',
    '../aiken/*',
    '../openvm-verifier/*',
    'fs',    // Node.js builtins used by emscripten (runtime-checked, never called in browser)
    'path',
  ],
}).catch(() => process.exit(1));
