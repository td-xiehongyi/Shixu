// Build only reviewed published KdbxWeb source; CJS imports current locked fflate.
const fs = require('node:fs');
const path = require('node:path');
const ts = require('typescript');
const base = __dirname;
const source = path.join(base, 'node_modules/kdbxweb/lib');
const out = path.join(base, 'kdbx-lib');
function build(dir) {
  for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, item.name);
    if (item.isDirectory()) { build(file); continue; }
    if (!file.endsWith('.ts')) continue;
    const target = path.join(out, path.relative(source, file).replace(/\.ts$/, '.js'));
    const result = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
      fileName: file,
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
      reportDiagnostics: true,
    });
    if (result.diagnostics.some(d => d.category === ts.DiagnosticCategory.Error)) throw Error('build failed');
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, result.outputText);
  }
}
fs.rmSync(out, { recursive: true, force: true });
build(source);
fs.writeFileSync(path.join(out, 'package.json'), '{"type":"commonjs"}\n');
