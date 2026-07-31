#!/usr/bin/env node
import { spawnSync } from 'node:child_process';

const forbiddenTerms = [
  'Robinhood',
  'Nitro',
  'Noxa',
  'chain4663',
  'EIP',
  'Alloy',
  'Ethereum-keystore',
  'Bankr',
  'Clanker',
  'Pons',
  'LaunchHood',
  'Flap',
  'Stonks',
  'Uniswap',
  'hermes-feed',
  '207.154.228.222',
];

const args = [
  'grep',
  '-n',
  '-I',
  ...forbiddenTerms.flatMap((term) => ['-e', term]),
  '--',
  ':(exclude)hermes-feed/**',
  ':(exclude)node_modules/**',
  '.'
];

const result = spawnSync('git', args, { encoding: 'utf8' });

if (result.status === 1) {
  console.log('[absence-gate] No forbidden Robinhood/EVM terms found in tracked repository files.');
  process.exit(0);
}

if (result.status !== 0) {
  console.error(result.stderr || result.stdout || 'Failed to run git grep for absence checks.');
  process.exit(result.status || 1);
}

const hits = result.stdout.trim();
const files = new Set(
  hits
    .split('\n')
    .map((line) => line.split(':')[0])
    .filter(Boolean)
);

console.error('[absence-gate] Forbidden terms were found in this tree:');
console.error(hits);
for (const file of [...files]) {
  console.error(`- ${file}`);
}
console.error(`
[absence-gate] Hard fail: remove or relocate EVM/Robinhood product material before merging.`);
process.exit(1);
