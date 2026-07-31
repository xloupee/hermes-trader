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
  ':(exclude)scripts/robinhood-absence-gate.mjs',
  '.'
];

const result = spawnSync('git', args, { encoding: 'utf8' });
let failed = false;

if (result.status === 1) {
  console.log('[absence-gate] No forbidden Robinhood/EVM terms found in tracked repository files.');
} else if (result.status !== 0) {
  console.error(result.stderr || result.stdout || 'Failed to run git grep for absence checks.');
  process.exit(result.status || 1);
} else {
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
  failed = true;
}

const remoteTransportArgs = [
  'grep',
  '-n',
  '-I',
  '-E',
  '-e',
  '(^|[^[:alnum:]_-])(ssh|scp|rsync)([^[:alnum:]_-]|$)',
  '--',
  '*.sh',
  'package.json',
  '.github/workflows/**',
];

const remoteTransportResult = spawnSync('git', remoteTransportArgs, { encoding: 'utf8' });

if (remoteTransportResult.status === 0) {
  console.error('[absence-gate] Remote transport commands were found in executable repository surfaces:');
  console.error(remoteTransportResult.stdout.trim());
  failed = true;
} else if (remoteTransportResult.status !== 1) {
  console.error(
    remoteTransportResult.stderr ||
      remoteTransportResult.stdout ||
      'Failed to scan executable repository surfaces for remote transport commands.'
  );
  process.exit(remoteTransportResult.status || 1);
} else {
  console.log('[absence-gate] No remote transport commands found in executable repository surfaces.');
}

if (failed) {
  process.exit(1);
}
