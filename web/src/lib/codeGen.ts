import type { DocLanguage } from "../data/apiDocs.ts";

export function formatCodeSnippet(lang: DocLanguage, url: string): string {
  switch (lang) {
    case "curl":
      return `curl -fsSL "${url}"`;
    case "javascript":
      return [
        `const response = await fetch("${url}");`,
        `if (!response.ok) throw new Error(\`HTTP \${response.status}\`);`,
        `const data = await response.json();`,
        `console.log(data);`
      ].join("\n");
    case "php":
      return [
        `$ch = curl_init("${url}");`,
        `curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);`,
        `$body = curl_exec($ch);`,
        `$data = json_decode($body, true);`
      ].join("\n");
    case "python":
      return [
        `import requests`,
        ``,
        `response = requests.get("${url}", timeout=10)`,
        `response.raise_for_status()`,
        `print(response.json())`
      ].join("\n");
    case "go":
      return [
        `resp, err := http.Get("${url}")`,
        `if err != nil { log.Fatal(err) }`,
        `defer resp.Body.Close()`,
        `var out map[string]any`,
        `json.NewDecoder(resp.Body).Decode(&out)`
      ].join("\n");
  }
}
