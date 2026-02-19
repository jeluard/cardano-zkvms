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
  plugins: [copyAssetsPlugin, copyHtmlPlugin],
  external: [
    '../uplc/*',
    '../aiken/*',
    '../openvm-verifier/*',
    'fs',    // Node.js builtins used by emscripten (runtime-checked, never called in browser)
    'path',
  ],
}).catch(() => process.exit(1));
