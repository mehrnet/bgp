import { For, Show, type JSX } from "solid-js";
import { CopyButton } from "./CopyButton.tsx";
import { CountryFlag } from "./CountryFlag.tsx";
import { Icon } from "./Icon.tsx";

export interface IdentityChip {
  text: string;
  accent?: boolean | undefined;
  countryCode?: string | null | undefined;
}

interface IdentityBannerProps {
  kickerIcon: string;
  kickerLabel: string;
  title: string;
  copyLabel: string;
  onCopied: (msg: string) => void;
  chips: IdentityChip[];
  rightLabel: string;
  rightHeading: JSX.Element | string;
  rightSubtitle?: JSX.Element | undefined;
  rightExtra?: JSX.Element | undefined;
  rightClass?: string | undefined;
}

export function IdentityBanner(props: IdentityBannerProps): JSX.Element {
  return (
    <section class="result-identity">
      <div class="address-block">
        <span class="section-label">
          <Icon id={props.kickerIcon} />
          <span>{props.kickerLabel}</span>
        </span>
        <div class="address-line">
          <h2>{props.title}</h2>
          <CopyButton
            text={props.title}
            label={props.copyLabel}
            onCopied={props.onCopied}
          />
        </div>
        <div class="identity-meta">
          <For each={props.chips}>
            {(chip) => (
              <span
                class="meta-chip"
                classList={{
                  accent: Boolean(chip.accent),
                  "with-flag": Boolean(chip.countryCode)
                }}
              >
                <CountryFlag code={chip.countryCode} />
                {chip.text}
              </span>
            )}
          </For>
        </div>
      </div>
      <div class={`network-block ${props.rightClass ?? ""}`}>
        <span class="section-label">{props.rightLabel}</span>
        <strong>{props.rightHeading}</strong>
        <Show when={props.rightSubtitle}>
          {(sub) => <div class="network-subtitle">{sub()}</div>}
        </Show>
        {props.rightExtra}
      </div>
    </section>
  );
}
