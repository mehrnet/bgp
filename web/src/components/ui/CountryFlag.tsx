import { Show, type JSX } from "solid-js";

interface CountryFlagProps {
  code?: string | null | undefined;
}

const countryNames =
  typeof Intl !== "undefined" && typeof Intl.DisplayNames === "function"
    ? new Intl.DisplayNames(["en"], { type: "region" })
    : null;

export function countryLabel(code?: string | null): string | null {
  const upper = code?.trim().toUpperCase();
  if (!upper || !/^[A-Z]{2}$/.test(upper)) return upper || null;
  try {
    return countryNames?.of(upper) ?? upper;
  } catch {
    return upper;
  }
}

export function CountryFlag(props: CountryFlagProps): JSX.Element {
  const normalized = () => {
    const raw = props.code?.trim().toLowerCase() ?? "";
    return /^[a-z]{2}$/.test(raw) ? raw : null;
  };

  return (
    <Show when={normalized()}>
      {(cc) => (
        <svg class="country-flag" viewBox="0 0 640 480" aria-hidden="true">
          <use href={`/icons.svg#flag-${cc()}`} />
        </svg>
      )}
    </Show>
  );
}
