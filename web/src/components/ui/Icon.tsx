import type { JSX } from "solid-js";

interface IconProps {
  id: string;
  class?: string;
}

export function Icon(props: IconProps): JSX.Element {
  const viewBox = () =>
    props.id === "icon-bgp-mark" ? "0 0 32 32" : "0 0 24 24";

  return (
    <svg class={props.class} viewBox={viewBox()} aria-hidden="true">
      <use href={`/icons.svg#${props.id}`} />
    </svg>
  );
}
