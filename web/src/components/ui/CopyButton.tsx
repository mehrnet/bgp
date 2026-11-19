import { createSignal, Show, type JSX } from "solid-js";
import { Icon } from "./Icon.tsx";

interface CopyButtonProps {
  text: string;
  label: string;
  variant?: "icon" | "code";
  onCopied?: (msg: string) => void;
}

async function copyToClipboard(value: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    return false;
  }
}

export function CopyButton(props: CopyButtonProps): JSX.Element {
  const [state, setState] = createSignal<"idle" | "copied" | "failed">("idle");
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function handleCopy() {
    const ok = await copyToClipboard(props.text);
    clearTimeout(timer);
    setState(ok ? "copied" : "failed");
    props.onCopied?.(ok ? `Copied ${props.text}` : "Unable to copy to clipboard");
    timer = setTimeout(() => setState("idle"), 1600);
  }

  const className = () =>
    props.variant === "code" ? "code-copy-button" : "copy-button";

  return (
    <button
      type="button"
      class={className()}
      data-copy-state={state() === "idle" ? undefined : state()}
      aria-label={props.label}
      title={props.label}
      onClick={handleCopy}
    >
      <Show
        when={state() === "copied"}
        fallback={<Icon id="icon-copy" />}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M5 12.5 9.5 17 19 7.5" />
        </svg>
      </Show>
      <Show when={props.variant === "code"}>
        <span>{state() === "copied" ? "Copied" : props.label}</span>
      </Show>
    </button>
  );
}
