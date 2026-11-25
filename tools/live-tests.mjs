// Runs the live test of `harness-gateway-client` that CheapSeek can serve,
// against a Gateway that this script starts and stops.
//
// Usage: node tools/live-tests.mjs <gateway>
//   <gateway>  the path of a `runvil-gateway` executable, such as the one in
//              a release of cppalliance/runvil-gateway
//
// Environment:
//   CHEAPSEEK_BASE_URL  the base URL of a running CheapSeek, such as
//                       http://127.0.0.1:54321/v1
//
// CheapSeek (https://github.com/cppalliance/cheapseek) is a test server that
// speaks the OpenAI chat API and computes each reply from fixed rules, so
// the test needs neither a model file nor a provider's key.
//
// The script makes a temporary home folder whose Gateway config serves
// CheapSeek as its one chat model, starts the Gateway there on a free
// loopback port, and runs the test with `RUNVIL_E2E_GATEWAY` and
// `RUNVIL_E2E_KEY` set. It stops the Gateway before every exit.
//
// The other live test of the crate stays with a person:
// `the_shipped_hello_prompt_ends_with_text_on_a_local_model_and_on_a_cloud_model`
// needs a model that the Gateway loads itself and a provider's paid key.
//
// Needs Node 22 or later and cargo-nextest, and no install.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, writeFileSync, writeSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

// The test this script runs, by its full name.
const TEST = "a_host_sends_the_shipped_hello_prompt_through_a_real_gateway";
// The key the Gateway requires of its callers. The Gateway listens on the
// loopback address only, for the length of one run.
const KEY = "live-tests";

// The Gateway this script started, once it has one.
let gateway;

// The Gateway runs in a process group of its own, because the program this
// script starts supervises a second process that serves.
function stop() {
  if (gateway) {
    try {
      process.kill(-gateway.pid, "SIGTERM");
    } catch {
      // The group is gone already.
    }
  }
}

// The line is written before the exit: a line queued on a pipe can be lost
// when the process exits.
function fail(message) {
  writeSync(2, `live-tests: ${message}\n`);
  stop();
  process.exit(1);
}

// Whatever ends the script, the Gateway is stopped first.
process.on("uncaughtException", (error) => fail(`stopped by an error: ${error?.stack ?? error}`));
process.on("unhandledRejection", (reason) => fail(`stopped by an error: ${reason?.stack ?? reason}`));
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => fail(`stopped by ${signal}`));
}

function freePort() {
  return new Promise((resolve) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

const [program] = process.argv.slice(2);
if (!program || process.argv.length !== 3) {
  fail(`usage: live-tests.mjs <gateway>; got ${process.argv.length - 2} arguments`);
}
if (!existsSync(program)) fail(`the Gateway executable ${program} does not exist`);
const cheapseek = process.env.CHEAPSEEK_BASE_URL;
if (!cheapseek) fail("CHEAPSEEK_BASE_URL must hold the base URL of a running CheapSeek; it is empty");

const home = mkdtempSync(join(tmpdir(), "runvil-live-"));
const port = await freePort();
mkdirSync(join(home, ".runvil"), { recursive: true });
writeFileSync(
  join(home, ".runvil", "gateway.toml"),
  `config-version = 0

[server]
bind = "127.0.0.1:${port}"
api_key = "${KEY}"
trust_loopback = false

[[endpoint]]
id = "cheapseek"
protocol = "openai"
base_url = "${cheapseek}"
api_key = ""

[[model]]
name = "cheapseek"
description = "CheapSeek, a test server that answers by rule"
context = 32768
upstream = "cheapseek"
endpoints = ["cheapseek"]

[[profile]]
name = "default"
models = []
`,
);

const log = join(home, "gateway.log");
const output = openSync(log, "a");
gateway = spawn(program, ["--no-tray", "--profile", "default"], {
  env: { ...process.env, HOME: home },
  cwd: home,
  detached: true,
  stdio: ["ignore", output, output],
});

// The Gateway is ready once it lists the model.
const root = `http://127.0.0.1:${port}/v1`;
let listed = "";
const end = Date.now() + 120_000;
while (!listed.includes('"cheapseek"')) {
  if (gateway.exitCode !== null) fail(`the Gateway exited with ${gateway.exitCode} while starting; see ${log}`);
  if (Date.now() > end) fail(`the Gateway must list the model cheapseek within 120 seconds; see ${log}`);
  await new Promise((resolve) => setTimeout(resolve, 500));
  try {
    const response = await fetch(`${root}/models`, {
      headers: { authorization: `Bearer ${KEY}` },
      signal: AbortSignal.timeout(5_000),
    });
    listed = await response.text();
  } catch {
    // The Gateway does not listen yet.
  }
}
console.log(`live-tests: a Gateway serves CheapSeek at ${root}`);

const run = spawnSync(
  "cargo",
  [
    "nextest", "run", "--locked", "-p", "harness-gateway-client", "--test", "live",
    "--run-ignored", "ignored-only", "-E", `test(=${TEST})`,
  ],
  { stdio: "inherit", env: { ...process.env, RUNVIL_E2E_GATEWAY: root, RUNVIL_E2E_KEY: KEY } },
);
stop();
if (run.status !== 0) fail(`the live test must pass; cargo nextest exited with ${run.status}`);
writeSync(1, "live-tests: passed\n");
process.exit(0);
