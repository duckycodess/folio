/**
 * The chat composer's slash commands, mapped to Folio's existing Search,
 * Organize and Summarize handlers. This is the only place that turns typed
 * text into one of these three actions: it is called on the literal text the
 * user submits, never on a retrieved passage or any other model output, so a
 * document's own words can never trigger a command.
 */
export type SlashCommandName = "search" | "organize" | "summarize";

export interface SlashCommandSpec {
  name: SlashCommandName;
  /** Shown in the suggestion list, e.g. "/search <query>". */
  usage: string;
  hint: string;
}

export const SLASH_COMMANDS: readonly SlashCommandSpec[] = [
  { name: "search", usage: "/search <query>", hint: "Find files" },
  { name: "organize", usage: "/organize", hint: "Open Organize" },
  {
    name: "summarize",
    usage: "/summarize [file]",
    hint: "Summarize the file, or ask which one",
  },
];

export interface ParsedCommand {
  name: SlashCommandName;
  /** Trimmed; "" when the command takes no argument or none was given. */
  args: string;
}

/**
 * Suggestions for what's typed so far after "/". Empty input suggests every
 * command; "/s" narrows to "search" and "summarize".
 */
export function matchSlashCommands(input: string): SlashCommandSpec[] {
  const typed = input.trim().toLocaleLowerCase();
  if (!typed) return [...SLASH_COMMANDS];
  return SLASH_COMMANDS.filter((command) => command.name.startsWith(typed));
}

/**
 * Parses one allowlisted command out of composer text, with its argument.
 * Anything not starting with "/", or not matching a known command name, is
 * not a command at all: it's an ordinary request to Ask Olio.
 */
export function parseCommand(text: string): ParsedCommand | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("/")) return null;
  const [word, ...rest] = trimmed.slice(1).split(/\s+/);
  const name = word?.toLocaleLowerCase();
  const command = SLASH_COMMANDS.find((candidate) => candidate.name === name);
  if (!command) return null;
  return { name: command.name, args: rest.join(" ").trim() };
}
