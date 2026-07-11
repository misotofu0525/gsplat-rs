import { createReadStream, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(harnessRoot, '..', '..', '..');
const configuredPort = Number(process.env.PLAYCANVAS_HARNESS_PORT ?? 4174);

const routes = [
  ['/vendor/playcanvas/', resolve(harnessRoot, 'node_modules/playcanvas/build/playcanvas')],
  ['/harness/', resolve(harnessRoot, 'public')],
  ['/datasets/', resolve(repoRoot, 'tests/datasets')]
];

const mime = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.ply', 'application/octet-stream']
]);

function routePath(url) {
  const pathname = new URL(url, 'http://127.0.0.1').pathname;
  if (pathname === '/') return resolve(harnessRoot, 'public/index.html');
  for (const [prefix, root] of routes) {
    if (!pathname.startsWith(prefix)) continue;
    const candidate = resolve(root, `.${sep}${pathname.slice(prefix.length)}`);
    if (candidate === root || candidate.startsWith(`${root}${sep}`)) return candidate;
  }
  return null;
}

export function startServer(port = configuredPort) {
  const server = createServer((request, response) => {
    try {
      const path = routePath(request.url ?? '/');
      if (!path || !statSync(path).isFile()) throw new Error('not found');
      response.writeHead(200, {
        'content-type': mime.get(extname(path)) ?? 'application/octet-stream',
        'cache-control': 'no-store',
        'cross-origin-resource-policy': 'same-origin'
      });
      createReadStream(path).pipe(response);
    } catch {
      response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
      response.end('not found');
    }
  });
  return new Promise((resolvePromise, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => {
      const address = server.address();
      resolvePromise({ server, port: typeof address === 'object' ? address.port : port });
    });
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { port: boundPort } = await startServer(configuredPort);
  console.log(`PlayCanvas harness: http://127.0.0.1:${boundPort}/`);
}
