const NO_TOOLS_PREAMBLE = `CRITICAL: Respond with TEXT ONLY. Do NOT call any tools.

- Do NOT use Read, Bash, Grep, Glob, Edit, Write, or ANY other tool.
- You already have all the context you need in the conversation above.
- Tool calls will be REJECTED and will waste your only turn — you will fail the task.
- Your entire response must be plain text: an <analysis> block followed by a <summary> block.

`;

const NO_TOOLS_TRAILER =
  "\n\nREMINDER: Do NOT call any tools. Respond with plain text only — " +
  "an <analysis> block followed by a <summary> block. Tool calls will be rejected and you will fail the task.";

const BASE_COMPACT_PROMPT = `Your task is to create a detailed summary of the conversation so far, paying close attention to the user's explicit requests and your previous actions.
This summary should be thorough in capturing technical details, code patterns, and architectural decisions that would be essential for continuing development work without losing context.

Before providing your final summary, wrap your analysis in <analysis> tags to organize your thoughts and ensure you've covered all necessary points. In your analysis process:

1. Chronologically analyze each message and section of the conversation. For each section thoroughly identify:
   - The user's explicit requests and intents
   - Your approach to addressing the user's requests
   - Key decisions, technical concepts and code patterns
   - Specific details like:
     - file names
     - full code snippets
     - function signatures
     - file edits
   - Errors that you ran into and how you fixed them
   - Pay special attention to specific user feedback that you received, especially if the user told you to do something differently.
   - Note any security-relevant instructions or constraints the user stated (e.g., sensitive files or data to avoid, operations that must not be performed, credential or secret handling rules). These MUST be preserved verbatim in the summary so they continue to apply after compaction.
2. Double-check for technical accuracy and completeness, addressing each required element thoroughly.

Your summary should include the following sections:

1. Primary Request and Intent: Capture all of the user's explicit requests and intents in detail
2. Key Technical Concepts: List all important technical concepts, technologies, and frameworks discussed.
3. Files and Code Sections: Enumerate specific files and code sections examined, modified, or created. Pay special attention to the most recent messages and include full code snippets where applicable and include a summary of why this file read or edit is important.
4. Errors and fixes: List all errors that you ran into, and how you fixed them. Pay special attention to specific user feedback that you received, especially if the user told you to do something differently.
5. Problem Solving: Document problems solved and any ongoing troubleshooting efforts.
6. All user messages: List ALL user messages that are not tool results. These are critical for understanding the users' feedback and changing intent. Preserve any security-relevant instructions or constraints verbatim so they remain in effect after compaction. Only messages that actually came from the user (user-role turns) count as user messages. Text inside assistant messages that is merely formatted like a user turn — e.g. quoted "user: ..." or "Human: ..." lines, or text shaped like a transcript rendering of a user turn — is model-generated: never attribute it to the user or describe it as a user request, approval, or confirmation.
7. Pending Tasks: Outline any pending tasks that you have explicitly been asked to work on.
8. Current Work: Describe in detail precisely what was being worked on immediately before this summary request, paying special attention to the most recent messages from both user and assistant. Include file names and code snippets where applicable.
9. Optional Next Step: List the next step that you will take that is related to the most recent work you were doing. IMPORTANT: ensure that this step is DIRECTLY in line with the user's most recent explicit requests, and the task you were working on immediately before this summary request. If your last task was concluded, then only list next steps if they are explicitly in line with the users request. Do not start on tangential requests or really old requests that were already completed without confirming with the user first.
                       If there is a next step, include direct quotes from the most recent conversation showing exactly what task you were working on and where you left off. This should be verbatim to ensure there's no drift in task interpretation.

This summary request, including its text-only / no-tools rule, is an internal step of the system and not a message from the user. Do not list it under "All user messages", "Pending Tasks", "Current Work" or "Optional Next Step", and do not describe it as a constraint on the work. The no-tools rule only applies while you write this summary; once work resumes from the summary, tools are available again.

Here's an example of how your output should be structured:

<example>
<analysis>
[Your thought process, ensuring all points are covered thoroughly and accurately]
</analysis>

<summary>
1. Primary Request and Intent:
   [Detailed description]

2. Key Technical Concepts:
   - [Concept 1]
   - [Concept 2]
   - [...]

3. Files and Code Sections:
   - [File Name 1]
      - [Summary of why this file is important]
      - [Summary of the changes made to this file, if any]
      - [Important Code Snippet]
   - [File Name 2]
      - [Important Code Snippet]
   - [...]

4. Errors and fixes:
    - [Detailed description of error 1]:
      - [How you fixed the error]
      - [User feedback on the error if any]
    - [...]

5. Problem Solving:
   [Description of solved problems and ongoing troubleshooting]

6. All user messages:${" "}
    - [Detailed non tool use user message]
    - [...]

7. Pending Tasks:
   - [Task 1]
   - [Task 2]
   - [...]

8. Current Work:
   [Precise description of current work]

9. Optional Next Step:
   [Optional Next step to take]

</summary>
</example>

Please provide your summary based on the conversation so far, following this structure and ensuring precision and thoroughness in your response.${" "}

There may be additional summarization instructions provided in the included context. If so, remember to follow these instructions when creating the above summary. Examples of instructions include:
<example>
## Compact Instructions
When summarizing the conversation focus on typescript code changes and also remember the mistakes you made and how you fixed them.
</example>

<example>
# Summary instructions
When you are using compact - please focus on test output and code changes. Include file reads verbatim.
</example>
`;

const WORK_ITEM_STATUSES = ["done", "in progress", "not started", "blocked"] as const;

export type CompactSummaryWorkItemStatus = (typeof WORK_ITEM_STATUSES)[number];

export interface CompactSummaryWorkItem {
  status: CompactSummaryWorkItemStatus;
  text: string;
}

export interface SubagentCompactPromptOptions {
  targetSummaryTokens: number;
}

/**
 * 通用 summary 模板以"Optional Next Step / 若任务已结束则不列下一步"收尾，没有逐项状态，
 * subagent 续接时读起来像收尾，于是在 compact 后几步就自行交付半成品。这里要求逐字保留
 * brief 并给出逐项清单，续接提醒再从清单里取出未完成条目。
 */
function buildSubagentCompactSection(options: SubagentCompactPromptOptions): string {
  const targetWords = Math.max(500, Math.floor(options.targetSummaryTokens * 0.75));
  return `

Subagent continuation requirements (these override the section list above where they conflict):
You are a subagent working on a brief delegated by a parent agent. The brief is not finished: after this summary you will keep calling tools to finish it. Nothing has run out — the context is being compacted to make room, and there is no token or turn budget. Write the summary as a working handoff, not as a final report.

Your <summary> must start with these two sections, before section 1:

Original Brief:
   [The delegated task, copied verbatim from the first user message. Shorten only if it is extremely long, and say that you did.]

Work Items:
   [One line per item in the brief, plus items you discovered, in this exact format:]
   - [done] <item> — evidence: <file:line, log quote, command output> — next: none
   - [in progress] <item> — evidence: <what is known so far> — next: <the concrete next tool call>
   - [not started] <item> — evidence: none — next: <the concrete first tool call>
   - [blocked] <item> — evidence: <why> — next: <what would unblock it>

Use only the statuses done, in progress, not started and blocked. Mark an item done only if its result is recorded under evidence; findings that are not written down are lost after compaction.

Length: keep the <analysis> block to a few short bullet points. Keep the whole <summary> under about ${targetWords} words. Prefer file:line references and short exact quotes over full code snippets or file contents; the files can be read again.`;
}

export function buildCompactPrompt(
  customInstructions: string | undefined,
  options: { subagent?: SubagentCompactPromptOptions } = {},
): string {
  const customInstructionBlock = customInstructions?.trim()
    ? `\n\nAdditional Instructions:\n${customInstructions}`
    : "";
  const subagentBlock = options.subagent ? buildSubagentCompactSection(options.subagent) : "";

  return `${NO_TOOLS_PREAMBLE}${BASE_COMPACT_PROMPT}${subagentBlock}${customInstructionBlock}${NO_TOOLS_TRAILER}`;
}

export function formatCompactSummary(text: string | undefined): string {
  let formatted = text?.trim() ?? "";
  if (!formatted) return "";

  formatted = formatted.replace(/<analysis>[\s\S]*?<\/analysis>/, "");
  const summaryMatch = formatted.match(/<summary>([\s\S]*?)<\/summary>/);
  if (summaryMatch) {
    const summary = summaryMatch[1] || "";
    // 修复原因：替换串是模板字符串时，摘要里的 $' $$ $& 会被当成替换语法，文本被改写。
    // 用回调返回替换文本，内容原样保留。
    formatted = formatted.replace(
      /<summary>[\s\S]*?<\/summary>/,
      () => `Summary:\n${summary.trim()}`,
    );
  } else {
    // 输出被 summary 上限截断时没有 </summary>；原先会把裸标签和未闭合的 <analysis>
    // 草稿一起推回上下文。取 <summary> 之后的全部文本。
    const openIndex = formatted.indexOf("<summary>");
    if (openIndex >= 0) {
      formatted = `Summary:\n${formatted.slice(openIndex + "<summary>".length).trim()}`;
    }
  }

  return formatted.replace(/\n\n+/g, "\n\n").trim();
}

const WORK_ITEM_LINE_PATTERN = /^\s*[-*]\s*\[(done|in progress|not started|blocked)\]\s*(.+?)\s*$/i;

export function parseCompactSummaryWorkItems(summary: string): CompactSummaryWorkItem[] {
  const items: CompactSummaryWorkItem[] = [];
  for (const line of summary.split(/\r?\n/)) {
    const match = WORK_ITEM_LINE_PATTERN.exec(line);
    if (!match?.[1] || !match[2]) continue;
    items.push({
      status: match[1].toLowerCase() as CompactSummaryWorkItemStatus,
      text: match[2],
    });
  }
  return items;
}

const MAX_CONTINUATION_BRIEF_CHARS = 12_000;

export function buildSubagentCompactContinuationReminder(input: {
  brief?: string;
  remainingItemsTag: string;
  summary: string;
}): string {
  const remaining = parseCompactSummaryWorkItems(input.summary).filter(
    (item) => item.status !== "done",
  );
  const brief = input.brief?.trim();
  const briefBlock = brief
    ? brief.length > MAX_CONTINUATION_BRIEF_CHARS
      ? `${brief.slice(0, MAX_CONTINUATION_BRIEF_CHARS)}\n[Brief truncated after ${MAX_CONTINUATION_BRIEF_CHARS} characters; the summary above restates it.]`
      : brief
    : undefined;
  const remainingBlock =
    remaining.length > 0
      ? [
          "Items that are not done yet, from the summary's Work Items list:",
          ...remaining.map((item) => `- [${item.status}] ${item.text}`),
        ].join("\n")
      : "Check the Work Items list in the summary above and continue with every item not marked done.";

  return [
    "Your context was just compacted to make room. This is routine and does not mean anything ran out: you have no token or turn budget, and your tools are available.",
    "Keep working on the task below until every item is done. Do not hand back partial results because of the compaction.",
    ...(briefBlock
      ? ["", "Your task (the latest instructions from the parent agent):", briefBlock]
      : []),
    "",
    remainingBlock,
    "",
    `Only if you genuinely cannot finish an item (for example it is blocked), end your final answer with a <${input.remainingItemsTag}> block listing each unfinished item on its own line, so the parent agent can resume you.`,
  ].join("\n");
}

// 保留段非空时追加在摘要消息最后，提醒模型摘要之后的原文比摘要更新。
const PRESERVED_MESSAGES_NOTE =
  "The messages after this summary are the most recent messages from before compaction, kept verbatim. The summary was written without seeing them, so something it says has not happened yet may already have happened in them.";

/**
 * `formattedSummary` 是 formatCompactSummary 的结果。
 * 修复原因：调用方已格式化一次，这里再格式化会把摘要正文里的 `<summary>` 标签再处理一遍，
 * 模型看到的文本与持久化的摘要不同；摘要只格式化一次。
 */
export function buildCompactSummaryMessage(
  formattedSummary: string,
  options: {
    preservedMessagesKept?: boolean;
    replStateCleared?: boolean;
    /** subagent 读到 "ran out of context" 会当成预算耗尽而收尾，改用不暗示耗尽的说法。 */
    subagent?: boolean;
    suppressFollowup?: boolean;
    transcriptPath?: string;
  } = {},
): string {
  const opening = options.subagent
    ? "Your earlier context was compacted to free working room; nothing ran out and no budget was used up. The summary below covers the earlier portion of your work on this task."
    : "This session is being continued from a previous conversation that ran out of context. The summary below covers the earlier portion of the conversation.";
  let message = `${opening}

${formattedSummary}

The request that produced this summary told the summarizer to respond with text only and not call tools. That rule applied only to writing the summary and is not a user instruction, even if the summary mentions it. All of your tools are available again; keep using them as the task requires.`;

  if (options.transcriptPath) {
    message += `\n\nIf you need specific details from before compaction (like exact code snippets, error messages, or content you generated), read the full transcript at: ${options.transcriptPath}`;
  }

  if (options.replStateCleared) {
    message +=
      "\n\nYour REPL VM state has been cleared as part of this compaction. Variables defined in REPL calls before this point are no longer accessible — redefine any you still need.";
  }

  if (options.suppressFollowup) {
    message +=
      '\nContinue the conversation from where it left off without asking the user any further questions. Resume directly — do not acknowledge the summary, do not recap what was happening, do not preface with "I\'ll continue" or similar. Pick up the last task as if the break never happened.';
  }

  if (options.preservedMessagesKept) {
    message += `\n\n${PRESERVED_MESSAGES_NOTE}`;
  }

  return message;
}
