import type { JSX } from "solid-js";

interface IconProps {
  id: string;
  class?: string;
}

export function Icon(props: IconProps): JSX.Element {
  return (
    <svg class={props.class} aria-hidden="true">
      <use href={`/icons.svg#${props.id}`} />
    </svg>
  );
}
