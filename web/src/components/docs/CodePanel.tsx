import type { JSX } from "solid-js";
import { CopyButton } from "../ui/CopyButton.tsx";

interface CodePanelProps {
  label: string;
  meta: string;
  code: string;
  variant?: "request" | "response";
  onCopied: (msg: string) => void;
}

export function CodePanel(props: CodePanelProps): JSX.Element {
  const isResponse = () => props.variant === "response";

  return (
    <div
      class="api-code-panel"
      classList={{ "api-response-panel": isResponse() }}
    >
      <div class="api-code-actions">
        <span class="api-code-label">{props.label}</span>
        <span class="api-code-type">{props.meta}</span>
        <CopyButton
          variant="code"
          text={props.code}
          label={`Copy ${props.label.toLowerCase()}`}
          onCopied={props.onCopied}
        />
      </div>
      <pre class="api-code" tabindex="0">
        <code>{props.code}</code>
      </pre>
    </div>
  );
}
