# upstash-arriba

[![Build](https://github.com/arriba-tools/upstash/actions/workflows/build.yml/badge.svg)](https://github.com/arriba-tools/upstash/actions/workflows/build.yml)
[![Lint](https://github.com/arriba-tools/upstash/actions/workflows/lint.yml/badge.svg)](https://github.com/arriba-tools/upstash/actions/workflows/lint.yml)
[![codecov](https://codecov.io/gh/arriba-tools/upstash/graph/badge.svg)](https://codecov.io/gh/arriba-tools/upstash)

An Upstash CLI for what [Upstash's own CLI](https://upstash.com/docs/agent-resources/cli) doesn't
cover: storing notes in an Upstash Vector index and searching them from a terminal. Upstash's CLI
manages indexes but has no data commands.

Coding agents on several machines use it to share findings. Each stores a note with `remember`,
and any of them finds it later with `recall`, by meaning or by the exact words, such as a figure.

## Install

Download a binary from the [releases](https://github.com/arriba-tools/upstash/releases) and put it
on your `PATH`, or build it from source:

```
cargo install --git https://github.com/arriba-tools/upstash
```

After that, `upstash-arriba upgrade` replaces the binary with the one from the latest release.

## Use with Claude

Add this line to `~/.claude/CLAUDE.md` so Claude Code reaches for `upstash-arriba` when a task
calls for shared notes:

```
* Use `upstash-arriba vector remember` and `upstash-arriba vector recall` for notes shared between agents. Run `upstash-arriba --help` for the commands.
```

## The index

The notes need an Upstash Vector index that embeds text itself, with hybrid search so that an exact
figure still finds its note.

1. In the [Upstash Console](https://console.upstash.com), on the **Vector** tab, click
   **Create Index**. Pick the type **Hybrid**, with a hosted embedding model for the dense part
   and **BM25** for the sparse part.
2. From the index's **Connect** section, copy `UPSTASH_VECTOR_REST_URL` and
   `UPSTASH_VECTOR_REST_TOKEN`. Take the token itself, not the read-only one, since `remember`
   writes.
3. Store them in `AGENT_VECTOR_URL` as one URL, with the token as `_token`, either in the
   environment or as an Actions variable of a repository that every machine's `gh` can read:

   ```
   gh variable set AGENT_VECTOR_URL -R owner/comms --body 'https://<endpoint>?_token=<token>'
   ```

4. Check that a machine reaches it. A new index has no notes.

   ```
   upstash-arriba -R owner/comms vector recall my-project test
   ```

The environment variable wins. Without it, the tool reads the Actions variable of the repository
that `-R` names, or of the one `gh` resolves in the working directory. Never print the value.

## Usage

```
upstash-arriba -R owner/comms vector remember linux my-project --text "Fits cross at D 236.6"
upstash-arriba -R owner/comms vector recall my-project "where do the fits cross" --count 5
upstash-arriba -R owner/comms vector remember linux my-project --replace <ID> --text "Fits cross at D 236.7"
upstash-arriba -R owner/comms vector forget my-project <ID> [<ID>...]
upstash-arriba upgrade
```

| Command | Description |
|---------|-------------|
| `vector remember <NAME> <NAMESPACE>` | Store a note from a `--file` or `--text`, and print its ID, or with `--replace` put it in place of a stored note |
| `vector recall <NAMESPACE> <QUERY>` | Print the namespace's notes that best match, by meaning and by keyword, best first |
| `vector forget <NAMESPACE> <ID>...` | Delete notes by ID |
| `upgrade` | Replace the binary with the one from the latest release |

A namespace holds one project's notes. A note's ID is the milliseconds since 1970, the name that
stored it, and the ID of the process that stored it. `recall` prints each note as a header, a blank
line, then its text. Its keyword half weighs each word by its rarity, so a figure such as `D 236.6`
outweighs common words.

`--replace` stores a correction under the ID of the note it corrects, so `recall` no longer finds
the old text. It refuses an ID that names no note in the namespace, rather than store a new one.

`forget` deletes the notes with the IDs given and says how many it deleted. When some ID named no
note, it says so and exits 1.

A run exits 0 when it succeeds, 1 when the index refuses a request or the run fails before sending
one, and 4 when the index can't be reached. Neither command retries.

## Development

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## License

MIT
