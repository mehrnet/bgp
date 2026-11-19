import type { JSX } from "solid-js";
import { queryToPath } from "../../lib/routes.ts";
import type { LookupQuery } from "../../types/api.ts";

interface QueryLinkProps {
  query: LookupQuery;
  label: string;
  onNavigate: (query: LookupQuery) => void;
}

export function QueryLink(props: QueryLinkProps): JSX.Element {
  function handleClick(event: MouseEvent) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    props.onNavigate(props.query);
  }

  return (
    <a
      class="query-link"
      href={queryToPath(props.query)}
      onClick={handleClick}
    >
      {props.label}
    </a>
  );
}
