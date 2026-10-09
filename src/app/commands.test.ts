import { describe, expect, it } from "vitest";
import { matchSlashCommands, parseCommand, SLASH_COMMANDS } from "./commands";

describe("parsing slash commands", () => {
  it("parses each allowlisted command with its argument", () => {
    expect(parseCommand("/search interview methods")).toEqual({
      name: "search",
      args: "interview methods",
    });
    expect(parseCommand("/organize")).toEqual({ name: "organize", args: "" });
    expect(parseCommand("/summarize notes.md")).toEqual({
      name: "summarize",
      args: "notes.md",
    });
    expect(parseCommand("/summarize")).toEqual({
      name: "summarize",
      args: "",
    });
  });

  it("is case-insensitive on the command name only", () => {
    expect(parseCommand("/SEARCH Interview Methods")).toEqual({
      name: "search",
      args: "Interview Methods",
    });
  });

  it("treats anything outside the registry as not a command", () => {
    expect(parseCommand("/delete notes.md")).toBeNull();
    expect(parseCommand("/organize-everything")).toBeNull();
    expect(parseCommand("search for notes")).toBeNull();
    expect(parseCommand("")).toBeNull();
  });

  it("never treats a retrieved passage as a command: only literal composer text is parsed", () => {
    // A passage from a document is shown as `passage.text`, which is never
    // passed to parseCommand anywhere in the app: only what the user types
    // into the composer is. This test fixes that one call site in the
    // registry's contract, so a document that happens to contain the words
    // "/organize" can't be re-interpreted as a command once quoted back.
    const passageText = "Remember to /organize the shared drive by Friday.";
    expect(parseCommand(passageText)).toBeNull();
  });
});

describe("slash suggestions", () => {
  it("suggests every command for an empty or bare slash", () => {
    expect(matchSlashCommands("")).toEqual([...SLASH_COMMANDS]);
  });

  it("narrows by the typed prefix", () => {
    expect(matchSlashCommands("s").map((c) => c.name)).toEqual([
      "search",
      "summarize",
    ]);
    expect(matchSlashCommands("org").map((c) => c.name)).toEqual(["organize"]);
  });

  it("suggests nothing for a prefix no command matches", () => {
    expect(matchSlashCommands("zzz")).toEqual([]);
  });
});
