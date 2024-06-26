// deno-fmt-ignore-file
// deno-lint-ignore-file

// Copyright Joyent and Node contributors. All rights reserved. MIT license.
// Taken from Node 18.12.1
// This file is automatically generated by `tests/node_compat/runner/setup.ts`. Do not modify this file manually.

'use strict';
const common = require('../common');

const { Writable } = require('stream');

{
  const w = new Writable({
    write(chunk, encoding, callback) {
      callback(null);
    },
    final(callback) {
      queueMicrotask(callback);
    }
  });
  w.end();
  w.destroy();

  w.on('prefinish', common.mustNotCall());
  w.on('finish', common.mustNotCall());
  w.on('close', common.mustCall());
}
