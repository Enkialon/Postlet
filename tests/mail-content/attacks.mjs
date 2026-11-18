/** Malicious mail HTML shared by browser and Tauri WebView probes. */
export const attacks = `
<p style="color:red;background-image:url(https://tracker.invalid/css)">safe body</p>
<script>parent.__TAURI_INTERNALS__.invoke('delete_messages'); window.__mailScriptRan = true;</script>
<img src="https://tracker.invalid/image" srcset="https://tracker.invalid/srcset 2x" onerror="parent.compromised=true">
<div background="https://tracker.invalid/background" style="background:url(https://tracker.invalid/shorthand)">CSS</div>
<style>@import url(https://tracker.invalid/import); @font-face{font-family:x;src:url(https://tracker.invalid/font)} body{background-image:url(https://tracker.invalid/style)}</style>
<iframe src="https://tracker.invalid/frame"></iframe><object data="https://tracker.invalid/object"></object>
<link rel="stylesheet" href="https://tracker.invalid/link"><meta http-equiv="refresh" content="0;url=https://tracker.invalid/refresh">
<form action="https://tracker.invalid/form"><input type="image" src="https://tracker.invalid/input"></form>
<svg><a xlink:href="javascript:parent.compromised=true">svg</a></svg>
<math><mtext><table><mglyph><style><!--</style><img title="--><img src=https://tracker.invalid/mutation onerror=parent.compromised=true>"></table></mtext></math>
<img src="cid:foreign-attachment"><img src="file:///C:/secret"><img src="asset://localhost/secret"><img src="data:image/svg+xml,<svg onload='parent.compromised=true'/>">
<a href="javascript:parent.compromised=true">bad link</a><a href="data:text/html,bad">data link</a>
<a href="https://example.com/path?q=1">website</a><a href="mailto:user@example.com">mail link</a>
`;
