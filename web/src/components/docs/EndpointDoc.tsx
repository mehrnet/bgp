import { For, type JSX } from "solid-js";
import type { DocLanguage, EndpointParam, EndpointSpec } from "../../data/apiDocs.ts";
import { formatCodeSnippet } from "../../lib/codeGen.ts";
import { Icon } from "../ui/Icon.tsx";
import { CodePanel } from "./CodePanel.tsx";

interface EndpointDocProps {
  spec: EndpointSpec;
  language: DocLanguage;
  onCopied: (msg: string) => void;
}

function ParameterTable(props: { parameters: EndpointParam[] }): JSX.Element {
  return (
    <div class="api-parameters">
      <h4>Parameters</h4>
      <div class="api-table-scroll" tabindex="0">
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>In</th>
              <th>Required</th>
              <th>Description</th>
            </tr>
          </thead>
          <tbody>
            <For each={props.parameters}>
              {(param) => (
                <tr>
                  <th><code>{param.name}</code></th>
                  <td>{param.location}</td>
                  <td>{param.required ? "Yes" : "No"}</td>
                  <td>{param.description}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function EndpointDoc(props: EndpointDocProps): JSX.Element {
  return (
    <article id={props.spec.id} class="api-example">
      <div class="api-example-header">
        <span class="api-waypoint"><Icon id="icon-route" /></span>
        <div>
          <h3>{props.spec.title}</h3>
          <p>{props.spec.summary}</p>
        </div>
      </div>
      <div class="api-service-body">
        <div class="api-endpoint-line">
          <span>{props.spec.method}</span>
          <code>{props.spec.pathTemplate}</code>
          <span class="meta-chip endpoint-tier">{props.spec.availability}</span>
        </div>
        <ParameterTable parameters={props.spec.parameters} />
        <div class="api-operation">
          <CodePanel
            label="Request"
            meta={props.language}
            code={formatCodeSnippet(props.language, props.spec.exampleUrl)}
            onCopied={props.onCopied}
          />
          <CodePanel
            label="Response"
            meta="application/json"
            variant="response"
            code={props.spec.responseJson}
            onCopied={props.onCopied}
          />
        </div>
      </div>
    </article>
  );
}
