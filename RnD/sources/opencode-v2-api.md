# OpenCode V2: API

Fetched from <https://opencode.ai/v2/docs/api/> on 2026-09-30, for the cctop OpenCode
hook work. Unedited apart from dropping the site's navigation around the
article body.

---

OpenAPI 3.1.0

# HTTP API

Experimental HttpApi surface for selected instance routes.

<div class="api-meta">

<span>136 operations</span><span>245 schemas</span>[OpenAPI JSON](/v2/openapi.json)

</div>

<div id="tag-server" class="section api-group">

Resource

## server

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/info`<span>Get server info</span>

<div class="api-operation-body">

Return the server identity, connection URLs, paths, and readiness status.

<span>Operation ID</span>`server.info`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>ServerInfo</span>

</div>

<div class="api-response-schema">

`application/json`[ServerInfo](#schema-ServerInfo)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-location" class="section api-group">

Resource

## location

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/location`<span>Get location</span>

<div class="api-operation-body">

Resolve the requested location or the server default location.

<span>Operation ID</span>`location.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Location.PublicInfo</span>

</div>

<div class="api-response-schema">

`application/json`[Location.PublicInfo](#schema-Location.PublicInfo)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/location/reload`<span>Reload configuration</span>

<div class="api-operation-body">

Shut down and rebuild every loaded location. Pending permissions and forms are cancelled; running sessions continue with fresh services at the next step boundary. Emits location.shutdown for client recovery and responds once all replacement builds settle.

<span>Operation ID</span>`location.reload`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-agent" class="section api-group">

Resource

## agent

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/agent`<span>List agents</span>

<div class="api-operation-body">

Retrieve currently registered agents.

<span>Operation ID</span>`agent.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Agent.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Agent.Info](#schema-Agent.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/agent/{agentID}`<span>Get agent</span>

<div class="api-operation-body">

Retrieve a single currently registered agent.

<span>Operation ID</span>`agent.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>agentID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Agent.Info](#schema-Agent.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>AgentNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[AgentNotFoundErrorEncoded](#schema-AgentNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-plugin" class="section api-group">

Resource

## plugin

Experimental plugin routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/plugin`<span>List plugins</span>

<div class="api-operation-body">

Retrieve enabled server plugins and their current status.

<span>Operation ID</span>`plugin.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Plugin.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Plugin.Info](#schema-Plugin.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/plugin/check`<span>Check plugin updates</span>

<div class="api-operation-body">

Check one or all package plugins for available updates.

<span>Operation ID</span>`plugin.check`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`target`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Plugin.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Plugin.Info](#schema-Plugin.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/plugin/update`<span>Update plugins</span>

<div class="api-operation-body">

Update package plugins concurrently and notify active locations to reload them. Responds once every update has finished; fails when any update fails.

<span>Operation ID</span>`plugin.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`targets`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-session" class="section api-group">

Resource

## session

Experimental session routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/session`<span>List sessions</span>

<div class="api-operation-body">

Retrieve sessions in the requested order. Items keep that order across pages; use cursor.next or cursor.previous to move through the ordered list.

<span>Operation ID</span>`session.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>limit</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<p>Maximum number of sessions to return. Defaults to the newest 50 sessions.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>order</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"asc" | "desc" | null</code>
</div>
<p>Session order for the first page. Use desc for newest first or asc for oldest first.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"asc" | "desc"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"asc" | "desc"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>search</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>parentID</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | "null" | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | "null"</code>
</div>
<p>Filter by parent session. Use null to return only root sessions.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"null"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"null"</code>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>directory</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>project</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>subpath</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>cursor</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<p>Opaque pagination cursor returned as cursor.previous or cursor.next in the previous response.</p>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>SessionsResponse</span>

</div>

<div class="api-response-schema">

`application/json`[SessionsResponse](#schema-SessionsResponse)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidCursorError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidCursorErrorEncoded | InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidCursorErrorEncoded](#schema-InvalidCursorErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session`<span>Create session</span>

<div class="api-operation-body">

Create a session at the requested location.

<span>Operation ID</span>`session.create`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`Model.Ref | null`

</div>

<div class="api-schema-variants">

<div>

[Model.Ref](#schema-Model.Ref)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`location`

<div class="api-schema-node">

<div class="api-schema-heading">

`Location.PublicRef | null`

</div>

<div class="api-schema-variants">

<div>

[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Metadata | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Metadata](#schema-Session.Metadata)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`permissions`

<div class="api-schema-node">

<div class="api-schema-heading">

`Permission.Ruleset | null`

</div>

<div class="api-schema-variants">

<div>

[Permission.Ruleset](#schema-Permission.Ruleset)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Info](#schema-Session.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/session/stats`<span>Get session statistics</span>

<div class="api-operation-body">

Aggregate local session activity, usage, and tool reliability for a time range.

<span>Operation ID</span>`experimental.session.stats`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>from</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>to</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>project</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>timezone</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>tools</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"none" | "summary" | "detail" | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"none" | "summary" | "detail"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"none" | "summary" | "detail"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[SessionStats.Info](#schema-SessionStats.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/session/import`<span>Import session</span>

<div class="api-operation-body">

Import a projected session transcript at the requested location. If parentID is supplied, the parent session must already exist; import parents before children.

<span>Operation ID</span>`experimental.session.import`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`info`[Session.Info](#schema-Session.Info)

</div>

<div class="api-schema-property">

`messages`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Session.Message.Info](#schema-Session.Message.Info)

</div>

</div>

</div>

<div class="api-schema-property">

`location`

<div class="api-schema-node">

<div class="api-schema-heading">

`Location.PublicRef | null`

</div>

<div class="api-schema-variants">

<div>

[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Info](#schema-Session.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>ConflictError</span>

</div>

<div class="api-response-schema">

`application/json`[ConflictErrorEncoded](#schema-ConflictErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/session/{sessionID}/export`<span>Export session</span>

<div class="api-operation-body">

Export a complete projected session transcript.

<span>Operation ID</span>`experimental.session.export`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>sanitize</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"true" | "false" | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"true" | "false"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"true" | "false"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[SessionTransfer.Data](#schema-SessionTransfer.Data)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>UnknownError</span>

</div>

<div class="api-response-schema">

`application/json`[UnknownErrorEncoded](#schema-UnknownErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/active`<span>List active sessions</span>

<div class="api-operation-body">

Retrieve foreground Session drains currently owned by this OpenCode process. Sessions absent from the result are inactive.

<span>Operation ID</span>`session.active`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}`<span>Get session</span>

<div class="api-operation-body">

Retrieve a session by ID.

<span>Operation ID</span>`session.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Info](#schema-Session.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-patch">patch</span>`/api/session/{sessionID}`<span>Update session</span>

<div class="api-operation-body">

Update mutable session properties.

<span>Operation ID</span>`session.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Metadata | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Metadata](#schema-Session.Metadata)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`permissions`

<div class="api-schema-node">

<div class="api-schema-heading">

`Permission.Ruleset | null`

</div>

<div class="api-schema-variants">

<div>

[Permission.Ruleset](#schema-Permission.Ruleset)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/session/{sessionID}`<span>Delete session</span>

<div class="api-operation-body">

Delete a session and its child sessions.

<span>Operation ID</span>`session.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/fork`<span>Fork session</span>

<div class="api-operation-body">

Create a child session by copying projected history before a message. Omit before to copy the full history.

<span>Operation ID</span>`session.fork`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`before`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Info](#schema-Session.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | MessageNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`MessageNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[MessageNotFoundErrorEncoded](#schema-MessageNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/agent`<span>Switch session agent</span>

<div class="api-operation-body">

Switch the agent used by subsequent provider turns.

<span>Operation ID</span>`session.switchAgent`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/model`<span>Switch session model</span>

<div class="api-operation-body">

Switch the model used by subsequent provider turns.

<span>Operation ID</span>`session.switchModel`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/move`<span>Move session</span>

<div class="api-operation-body">

Move a session to another project directory at the requested delivery boundary.

<span>Operation ID</span>`session.move`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`delivery`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.Delivery | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/prompt`<span>Send message</span>

<div class="api-operation-body">

Durably admit one session input and schedule agent-loop execution unless resume is false.

<span>Operation ID</span>`session.prompt`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`PromptInput.FileAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[PromptInput.FileAttachment](#schema-PromptInput.FileAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`agents`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.AgentAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.AgentAttachment](#schema-Prompt.AgentAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`skills`

<div class="api-schema-node">

<div class="api-schema-heading">

`PromptInput.SkillAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[PromptInput.SkillAttachment](#schema-PromptInput.SkillAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`delivery`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.Delivery | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`resume`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Inbox.User](#schema-Session.Inbox.User)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>ConflictError</span>

</div>

<div class="api-response-schema">

`application/json`[ConflictErrorEncoded](#schema-ConflictErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/command`<span>Run command</span>

<div class="api-operation-body">

Execute a slash command callback immediately.

<span>Operation ID</span>`session.command`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`PromptInput.FileAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[PromptInput.FileAttachment](#schema-PromptInput.FileAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`agents`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.AgentAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.AgentAttachment](#schema-Prompt.AgentAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`skills`

<div class="api-schema-node">

<div class="api-schema-heading">

`PromptInput.SkillAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[PromptInput.SkillAttachment](#schema-PromptInput.SkillAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`delivery`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.Delivery | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | CommandNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`CommandNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[CommandNotFoundErrorEncoded](#schema-CommandNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>CommandExecutionError</span>

</div>

<div class="api-response-schema">

`application/json`[CommandExecutionErrorEncoded](#schema-CommandExecutionErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/session/{sessionID}/skill`<span>Activate skill</span>

<div class="api-operation-body">

Activate a skill for a session by appending a skill message and resuming execution.

<span>Operation ID</span>`experimental.session.skill`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`resume`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | SkillNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SkillNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SkillNotFoundErrorEncoded](#schema-SkillNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/synthetic`<span>Add synthetic message</span>

<div class="api-operation-body">

Durably admit synthetic session input and schedule execution unless resume is false.

<span>Operation ID</span>`session.synthetic`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`delivery`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.Delivery | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`resume`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Inbox.Synthetic](#schema-Session.Inbox.Synthetic)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>ConflictError</span>

</div>

<div class="api-response-schema">

`application/json`[ConflictErrorEncoded](#schema-ConflictErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/shell`<span>Run shell command</span>

<div class="api-operation-body">

Execute one shell command in the session's working directory. Emits a shell.started event before execution and a shell.ended event with the merged output after.

<span>Operation ID</span>`session.shell`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/compact`<span>Compact session</span>

<div class="api-operation-body">

Durably admit a session compaction request. Steers by default: it runs at the next step boundary instead of waiting behind queued prompts.

<span>Operation ID</span>`session.compact`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`delivery`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.Delivery | null`

</div>

<div class="api-schema-variants">

<div>

[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Inbox.Compaction](#schema-Session.Inbox.Compaction)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>ConflictError</span>

</div>

<div class="api-response-schema">

`application/json`[ConflictErrorEncoded](#schema-ConflictErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/session/{sessionID}/wait`<span>Wait for session</span>

<div class="api-operation-body">

Wait for a session agent loop to become idle.

<span>Operation ID</span>`experimental.session.wait`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/revert/stage`<span>Stage session revert</span>

<div class="api-operation-body">

Stage or move a reversible session boundary and optionally apply its file changes.

<span>Operation ID</span>`session.revert.stage`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`messageID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Revert](#schema-Session.Revert)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>MessageNotFoundError | SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`MessageNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[MessageNotFoundErrorEncoded](#schema-MessageNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>SessionBusyError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionBusyErrorEncoded](#schema-SessionBusyErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>UnknownError</span>

</div>

<div class="api-response-schema">

`application/json`[UnknownErrorEncoded](#schema-UnknownErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/session/{sessionID}/revert`<span>Clear staged revert</span>

<div class="api-operation-body">

<span>Operation ID</span>`session.revert.clear`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>SessionBusyError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionBusyErrorEncoded](#schema-SessionBusyErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>UnknownError</span>

</div>

<div class="api-response-schema">

`application/json`[UnknownErrorEncoded](#schema-UnknownErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/revert/commit`<span>Commit staged revert</span>

<div class="api-operation-body">

<span>Operation ID</span>`session.revert.commit`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>SessionBusyError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionBusyErrorEncoded](#schema-SessionBusyErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/context`<span>Get session context</span>

<div class="api-operation-body">

Retrieve the active context messages for a session (all messages after the last compaction).

<span>Operation ID</span>`session.context`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Session.Message.Info](#schema-Session.Message.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>UnknownError</span>

</div>

<div class="api-response-schema">

`application/json`[UnknownErrorEncoded](#schema-UnknownErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/diff`<span>Diff session turns</span>

<div class="api-operation-body">

Structured per-file diffs of the files a turn changed. A turn runs from the first prompt after the session was last idle until its next idle marker, so prompts steered in while it was busy belong to the same turn; \`to\` extends the range through a later turn. Compares the range's first recorded snapshot with its last; a step still running in the active session compares against the working copy. Ranges that span a location change are rejected. In sessions without any idle marker, a prompt's turn spans until the next user message.

<span>Operation ID</span>`session.diff`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>from</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<p>User message whose turn to diff. Defaults to the turn of the newest user message.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^msg_
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>to</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<p>Later user message whose turn ends the range. Defaults to the turn of `from` alone.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^msg_
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>context</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<p>Unchanged lines around each hunk. Omit for full-file patches.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`FileDiff.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[FileDiff.Info](#schema-FileDiff.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>MessageNotFoundError | SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`MessageNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[MessageNotFoundErrorEncoded](#schema-MessageNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>UnknownError</span>

</div>

<div class="api-response-schema">

`application/json`[UnknownErrorEncoded](#schema-UnknownErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/inbox`<span>List session inbox</span>

<div class="api-operation-body">

List durable enqueued session work not yet delivered, ordered by enqueue sequence. Includes user, synthetic, compaction, and move items.

<span>Operation ID</span>`session.inbox.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Session.Inbox.Info](#schema-Session.Inbox.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-patch">patch</span>`/api/session/{sessionID}/inbox/{inboxID}`<span>Update inbox item</span>

<div class="api-operation-body">

Change a pending inbox item's delivery mode. Steering wakes session execution.

<span>Operation ID</span>`session.inbox.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>inboxID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^msg_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`delivery`[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>ConflictError</span>

</div>

<div class="api-response-schema">

`application/json`[ConflictErrorEncoded](#schema-ConflictErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/session/{sessionID}/inbox/{inboxID}`<span>Cancel inbox input</span>

<div class="api-operation-body">

Cancel an inbox item that has not yet been delivered. Unavailable items are a no-op.

<span>Operation ID</span>`session.inbox.cancel`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>inboxID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^msg_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/session/{sessionID}/instructions/entries`<span>List instruction entries</span>

<div class="api-operation-body">

List API-managed instruction entries attached to the session.

<span>Operation ID</span>`experimental.session.instructions.entry.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`InstructionEntry.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[InstructionEntry.Info](#schema-InstructionEntry.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-put">put</span>`/api/experimental/session/{sessionID}/instructions/entries/{key}`<span>Put instruction entry</span>

<div class="api-operation-body">

Attach or replace one durable instruction entry. Changes announce as updates at the next step boundary.

<span>Operation ID</span>`experimental.session.instructions.entry.put`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>key</code><span class="api-required">required</span></td>
<td>path</td>
<td><a href="#schema-InstructionEntry.Key" class="api-schema-link">InstructionEntry.Key</a></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`value`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`413`<span>InstructionEntryValueTooLargeError</span>

</div>

<div class="api-response-schema">

`application/json`[InstructionEntryValueTooLargeErrorEncoded](#schema-InstructionEntryValueTooLargeErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/experimental/session/{sessionID}/instructions/entries/{key}`<span>Remove instruction entry</span>

<div class="api-operation-body">

Remove one instruction entry; the removal is announced to the model at the next step boundary.

<span>Operation ID</span>`experimental.session.instructions.entry.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>key</code><span class="api-required">required</span></td>
<td>path</td>
<td><a href="#schema-InstructionEntry.Key" class="api-schema-link">InstructionEntry.Key</a></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/generate`<span>Generate text from session context</span>

<div class="api-operation-body">

Generate transient text from the current session context without mutating session history.

<span>Operation ID</span>`session.generate`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`prompt`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>SessionGenerateResponse</span>

</div>

<div class="api-response-schema">

`application/json`[SessionGenerateResponse](#schema-SessionGenerateResponse)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/session/{sessionID}/log`<span>Read the session log</span>

<div class="api-operation-body">

Experimental durable session event log. Reads events after an exclusive aggregate sequence and continues with live events when follow=true.

<span>Operation ID</span>`session.log`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>after</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>follow</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"true" | "false" | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"true" | "false"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"true" | "false"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`text/event-stream`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`event`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`data`[SessionLogItemEncoded](#schema-SessionLogItemEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/interrupt`<span>Interrupt session execution</span>

<div class="api-operation-body">

Interrupt active execution owned by this OpenCode process. Returns interrupted=true when an active execution was interrupted and false for the idle no-op. When resume=true, execution resumes pending steering input and next-in-line control items (manual compaction, moves) while queued prompts remain parked.

<span>Operation ID</span>`session.interrupt`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>resume</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"true" | "false" | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"true" | "false"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"true" | "false"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>SessionInterruptResponse</span>

</div>

<div class="api-response-schema">

`application/json`[SessionInterruptResponse](#schema-SessionInterruptResponse)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/background`<span>Background blocking session tools</span>

<div class="api-operation-body">

Move active foreground backgroundable tools for this session into background observation. Idle requests are a no-op.

<span>Operation ID</span>`session.background`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/message/{messageID}`<span>Get session message</span>

<div class="api-operation-body">

Retrieve one projected message owned by the Session.

<span>Operation ID</span>`session.message.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>messageID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^msg_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Session.Message.Info](#schema-Session.Message.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | MessageNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | MessageNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[MessageNotFoundErrorEncoded](#schema-MessageNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/form`<span>List session forms</span>

<div class="api-operation-body">

Retrieve pending forms for a session.

<span>Operation ID</span>`session.form.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Info](#schema-Form.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/form`<span>Create session form</span>

<div class="api-operation-body">

Create a form for a session.

<span>Operation ID</span>`session.form.create`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[Form.CreatePayload](#schema-Form.CreatePayload)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Form.Info](#schema-Form.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>ConflictError</span>

</div>

<div class="api-response-schema">

`application/json`[ConflictErrorEncoded](#schema-ConflictErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/form/{formID}`<span>Get session form</span>

<div class="api-operation-body">

Retrieve a form and its current state for a session.

<span>Operation ID</span>`session.form.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>formID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^frm_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Form.Detail](#schema-Form.Detail)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | FormNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`FormNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[FormNotFoundErrorEncoded](#schema-FormNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/session/{sessionID}/form/{formID}`<span>Cancel form</span>

<div class="api-operation-body">

Cancel a pending form.

<span>Operation ID</span>`session.form.cancel`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>formID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^frm_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | FormNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`FormNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[FormNotFoundErrorEncoded](#schema-FormNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>FormAlreadySettledError</span>

</div>

<div class="api-response-schema">

`application/json`[FormAlreadySettledErrorEncoded](#schema-FormAlreadySettledErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/form/{formID}/reply`<span>Reply to form</span>

<div class="api-operation-body">

Submit an answer to a pending form.

<span>Operation ID</span>`session.form.reply`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>formID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^frm_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[Form.Reply](#schema-Form.Reply)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>FormInvalidAnswerError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`FormInvalidAnswerErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[FormInvalidAnswerErrorEncoded](#schema-FormInvalidAnswerErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | FormNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`FormNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[FormNotFoundErrorEncoded](#schema-FormNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`409`<span>FormAlreadySettledError</span>

</div>

<div class="api-response-schema">

`application/json`[FormAlreadySettledErrorEncoded](#schema-FormAlreadySettledErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-put">put</span>`/api/session/{sessionID}/environment`<span>Set session environment</span>

<div class="api-operation-body">

Replace the process environment used by local shell commands for this session.

<span>Operation ID</span>`session.environment`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`variables`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/view`<span>View session</span>

<div class="api-operation-body">

Mark the idle transition observed by the viewer as viewed.

<span>Operation ID</span>`session.view`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`idle`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/message`<span>Get session messages</span>

<div class="api-operation-body">

Retrieve projected messages for a session, optionally filtered by type. Items keep the requested order across pages; use cursor.next or cursor.previous to move through the ordered timeline, passing the same type filter on each page.

<span>Operation ID</span>`session.message.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>limit</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<p>Maximum number of messages to return. When omitted, the endpoint returns its default page size.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>order</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"asc" | "desc" | null</code>
</div>
<p>Message order for the first page. Use desc for newest first or asc for oldest first.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"asc" | "desc"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"asc" | "desc"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>cursor</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<p>Opaque pagination cursor returned as cursor.previous or cursor.next in the previous response. Do not combine with order.</p>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>type</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"agent-switched" | "model-switched" | "location-switched" | "user" | "synthetic" | "system" | "skill" | "shell" | "assistant" | "compaction" | null</code>
</div>
<p>Filter by message type before pagination. When omitted, all message types are returned. Pass the same type when following cursors.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>"agent-switched" | "model-switched" | "location-switched" | "user" | "synthetic" | "system" | "skill" | "shell" | "assistant" | "compaction"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"agent-switched" | "model-switched" | "location-switched" | "user" | "synthetic" | "system" | "skill" | "shell" | "assistant" | "compaction"</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>SessionMessagesResponse</span>

</div>

<div class="api-response-schema">

`application/json`[SessionMessagesResponse](#schema-SessionMessagesResponse)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidCursorError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidCursorErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidCursorErrorEncoded](#schema-InvalidCursorErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>UnknownError</span>

</div>

<div class="api-response-schema">

`application/json`[UnknownErrorEncoded](#schema-UnknownErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-model" class="section api-group">

Resource

## model

Experimental model routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/model`<span>List models</span>

<div class="api-operation-body">

Retrieve the current snapshot of available models ordered by release date. The snapshot may precede initial plugin settlement.

<span>Operation ID</span>`model.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Model.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Model.Info](#schema-Model.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/model/default`<span>Get default model</span>

<div class="api-operation-body">

Retrieve the model used when a session has no explicit model selection.

<span>Operation ID</span>`model.default`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Model.Info | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[Model.Info](#schema-Model.Info)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-generate" class="section api-group">

Resource

## generate

Experimental one-shot generation routes.

<div class="api-operations">

<span class="api-method api-method-post">post</span>`/api/experimental/generate`<span>Generate text</span>

<div class="api-operation-body">

Run one stateless model generation using the server's base configuration and return the assistant text. Uses the base configuration's default model when none is specified.

<span>Operation ID</span>`experimental.generate.text`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`prompt`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`Model.Ref | null`

</div>

<div class="api-schema-variants">

<div>

[Model.Ref](#schema-Model.Ref)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>GenerateTextResponse</span>

</div>

<div class="api-response-schema">

`application/json`[GenerateTextResponse](#schema-GenerateTextResponse)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-provider" class="section api-group">

Resource

## provider

Experimental provider routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/provider`<span>List providers</span>

<div class="api-operation-body">

Retrieve active AI providers so clients can show provider availability and configuration.

<span>Operation ID</span>`provider.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Provider.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Provider.Info](#schema-Provider.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/provider/{providerID}`<span>Get provider</span>

<div class="api-operation-body">

Retrieve a single AI provider so clients can inspect its availability and endpoint settings.

<span>Operation ID</span>`provider.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>providerID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Provider.Info](#schema-Provider.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ProviderNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ProviderNotFoundErrorEncoded](#schema-ProviderNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-integration" class="section api-group">

Resource

## integration

Integration discovery and authentication routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/integration`<span>List integrations</span>

<div class="api-operation-body">

Retrieve available integrations and their authentication methods.

<span>Operation ID</span>`integration.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Integration.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Integration.Info](#schema-Integration.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/integration/{integrationID}`<span>Get integration</span>

<div class="api-operation-body">

Retrieve one integration and its authentication methods.

<span>Operation ID</span>`integration.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Integration.Info](#schema-Integration.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>IntegrationNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[IntegrationNotFoundErrorEncoded](#schema-IntegrationNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/integration/wellknown`<span>Add wellknown integration</span>

<div class="api-operation-body">

Discover and persist an experimental wellknown integration source.

<span>Operation ID</span>`experimental.integration.wellknown.add`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/integration/{integrationID}/connect/key`<span>Connect with key</span>

<div class="api-operation-body">

Run a key authentication method and store the resulting credential.

<span>Operation ID</span>`integration.connect.key`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`answer`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Answer | null`

</div>

<div class="api-schema-variants">

<div>

[Form.Answer](#schema-Form.Answer)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>IntegrationNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[IntegrationNotFoundErrorEncoded](#schema-IntegrationNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/integration/{integrationID}/connect/oauth`<span>Begin OAuth connection</span>

<div class="api-operation-body">

Start an OAuth attempt and return the authorization details.

<span>Operation ID</span>`integration.oauth.connect`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`methodID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`answer`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Answer | null`

</div>

<div class="api-schema-variants">

<div>

[Form.Answer](#schema-Form.Answer)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Integration.AttemptEncoded](#schema-Integration.AttemptEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/integration/{integrationID}/connect/oauth/{attemptID}`<span>Get OAuth attempt status</span>

<div class="api-operation-body">

Poll the current status of an OAuth attempt.

<span>Operation ID</span>`integration.oauth.status`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>attemptID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Integration.AttemptStatus](#schema-Integration.AttemptStatus)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>IntegrationNotFoundError | IntegrationAttemptNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`IntegrationNotFoundErrorEncoded | IntegrationAttemptNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[IntegrationNotFoundErrorEncoded](#schema-IntegrationNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[IntegrationAttemptNotFoundErrorEncoded](#schema-IntegrationAttemptNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/integration/{integrationID}/connect/oauth/{attemptID}`<span>Cancel OAuth connection</span>

<div class="api-operation-body">

Cancel an OAuth attempt and release its resources.

<span>Operation ID</span>`integration.oauth.cancel`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>attemptID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/integration/{integrationID}/connect/oauth/{attemptID}/complete`<span>Complete OAuth connection</span>

<div class="api-operation-body">

Complete a code-based OAuth attempt and store the resulting credential.

<span>Operation ID</span>`integration.oauth.complete`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>attemptID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`code`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>IntegrationNotFoundError | IntegrationAttemptNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`IntegrationNotFoundErrorEncoded | IntegrationAttemptNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[IntegrationNotFoundErrorEncoded](#schema-IntegrationNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[IntegrationAttemptNotFoundErrorEncoded](#schema-IntegrationAttemptNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/integration/{integrationID}/connect/command`<span>Begin command connection</span>

<div class="api-operation-body">

Start a command authentication attempt.

<span>Operation ID</span>`integration.command.connect`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`methodID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Integration.CommandAttempt](#schema-Integration.CommandAttempt)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>IntegrationNotFoundError | IntegrationMethodNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`IntegrationNotFoundErrorEncoded | IntegrationMethodNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[IntegrationNotFoundErrorEncoded](#schema-IntegrationNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[IntegrationMethodNotFoundErrorEncoded](#schema-IntegrationMethodNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/integration/{integrationID}/connect/command/{attemptID}`<span>Get command attempt status</span>

<div class="api-operation-body">

Poll the current status and output of a command authentication attempt.

<span>Operation ID</span>`integration.command.status`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>attemptID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Integration.CommandAttemptStatus](#schema-Integration.CommandAttemptStatus)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>IntegrationNotFoundError | IntegrationAttemptNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`IntegrationNotFoundErrorEncoded | IntegrationAttemptNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[IntegrationNotFoundErrorEncoded](#schema-IntegrationNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[IntegrationAttemptNotFoundErrorEncoded](#schema-IntegrationAttemptNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/integration/{integrationID}/connect/command/{attemptID}`<span>Cancel command connection</span>

<div class="api-operation-body">

Cancel a command authentication attempt and terminate its process.

<span>Operation ID</span>`integration.command.cancel`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>integrationID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>attemptID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-mcp" class="section api-group">

Resource

## mcp

MCP server and resource routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/mcp`<span>List MCP servers</span>

<div class="api-operation-body">

Retrieve configured MCP servers and their connection status.

<span>Operation ID</span>`mcp.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.Server[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Mcp.Server](#schema-Mcp.Server)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-put">put</span>`/api/experimental/mcp/{server}`<span>Add MCP server</span>

<div class="api-operation-body">

Add an MCP server at runtime or replace an existing one, connecting it immediately.

<span>Operation ID</span>`experimental.mcp.add`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>server</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`config`

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.LocalConfigEncoded | Mcp.RemoteConfigEncoded`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[Mcp.LocalConfigEncoded](#schema-Mcp.LocalConfigEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[Mcp.RemoteConfigEncoded](#schema-Mcp.RemoteConfigEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/experimental/mcp/{server}`<span>Remove MCP server</span>

<div class="api-operation-body">

Stop an MCP server and remove it from the runtime set until restart.

<span>Operation ID</span>`experimental.mcp.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>server</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>McpServerNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[McpServerNotFoundErrorEncoded](#schema-McpServerNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/mcp/{server}/connect`<span>Connect MCP server</span>

<div class="api-operation-body">

Connect an MCP server at runtime, overriding a disabled configuration until restart.

<span>Operation ID</span>`experimental.mcp.connect`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>server</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>McpServerNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[McpServerNotFoundErrorEncoded](#schema-McpServerNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/mcp/{server}/disconnect`<span>Disconnect MCP server</span>

<div class="api-operation-body">

Disconnect an MCP server at runtime, removing its tools until reconnected.

<span>Operation ID</span>`experimental.mcp.disconnect`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>server</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>McpServerNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[McpServerNotFoundErrorEncoded](#schema-McpServerNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/mcp/resource`<span>List MCP resources</span>

<div class="api-operation-body">

Retrieve resources and resource templates from connected MCP servers.

<span>Operation ID</span>`mcp.resource.catalog`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Mcp.ResourceCatalog](#schema-Mcp.ResourceCatalog)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-credential" class="section api-group">

Resource

## credential

<div class="api-operations">

<span class="api-method api-method-patch">patch</span>`/api/credential/{credentialID}`<span>Update credential</span>

<div class="api-operation-body">

Update a stored credential label.

<span>Operation ID</span>`credential.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>credentialID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/credential/{credentialID}`<span>Remove credential</span>

<div class="api-operation-body">

Remove a stored integration credential.

<span>Operation ID</span>`credential.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>credentialID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/credential/{credentialID}/activate`<span>Activate credential</span>

<div class="api-operation-body">

Activate a stored integration credential.

<span>Operation ID</span>`credential.activate`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>credentialID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-project" class="section api-group">

Resource

## project

Project routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/project`<span>List projects</span>

<div class="api-operation-body">

List known projects.

<span>Operation ID</span>`project.list`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`Project[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Project](#schema-Project)

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-patch">patch</span>`/api/project/{projectID}`<span>Update project</span>

<div class="api-operation-body">

Update the project canonical directory, display metadata, and workspace commands.

<span>Operation ID</span>`project.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>projectID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`canonical`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`icon`[Project.Icon](#schema-Project.Icon)

</div>

<div class="api-schema-property">

`commands`[Project.Commands](#schema-Project.Commands)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Project</span>

</div>

<div class="api-response-schema">

`application/json`[Project](#schema-Project)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ProjectNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ProjectNotFoundErrorEncoded](#schema-ProjectNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-form" class="section api-group">

Resource

## form

Location form routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/form`<span>List pending forms</span>

<div class="api-operation-body">

Retrieve pending forms for a location.

<span>Operation ID</span>`form.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Info](#schema-Form.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-permission" class="section api-group">

Resource

## permission

Experimental permission routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/permission/request`<span>List pending permission requests</span>

<div class="api-operation-body">

Retrieve pending permission requests for a location.

<span>Operation ID</span>`permission.request.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Permission.Request[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Permission.Request](#schema-Permission.Request)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/permission/saved`<span>List saved permissions</span>

<div class="api-operation-body">

Retrieve saved permissions, optionally filtered by project.

<span>Operation ID</span>`permission.saved.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>projectID</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`PermissionSaved.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[PermissionSaved.Info](#schema-PermissionSaved.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/permission/saved/{id}`<span>Remove saved permission</span>

<div class="api-operation-body">

Remove a saved permission by ID.

<span>Operation ID</span>`permission.saved.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>id</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/permission`<span>List session permission requests</span>

<div class="api-operation-body">

Retrieve pending permission requests owned by a session.

<span>Operation ID</span>`session.permission.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Permission.Request[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Permission.Request](#schema-Permission.Request)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/permission`<span>Create permission request</span>

<div class="api-operation-body">

Evaluate and, when approval is required, create a permission request for a session.

<span>Operation ID</span>`session.permission.create`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^per

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`action`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`resources`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`save`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`source`[Permission.Source](#schema-Permission.Source)

</div>

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^per

</div>

</div>

</div>

<div class="api-schema-property">

`effect`[Permission.Effect](#schema-Permission.Effect)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/session/{sessionID}/permission/{requestID}`<span>Get permission request</span>

<div class="api-operation-body">

Retrieve a pending permission request owned by a session.

<span>Operation ID</span>`session.permission.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>requestID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^per
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Permission.Request](#schema-Permission.Request)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | PermissionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`PermissionNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[PermissionNotFoundErrorEncoded](#schema-PermissionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/session/{sessionID}/permission/{requestID}/reply`<span>Reply to pending permission request</span>

<div class="api-operation-body">

Respond to a pending permission request owned by a session.

<span>Operation ID</span>`session.permission.reply`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>requestID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^per
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`decision`[Permission.Reply](#schema-Permission.Reply)

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>SessionNotFoundError | PermissionNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`PermissionNotFoundErrorEncoded | SessionNotFoundErrorEncoded | SessionNotFoundErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[PermissionNotFoundErrorEncoded](#schema-PermissionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[SessionNotFoundErrorEncoded](#schema-SessionNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-filesystem" class="section api-group">

Resource

## filesystem

Experimental location-scoped filesystem routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/fs/read/*`<span>Read file</span>

<div class="api-operation-body">

Serve one file relative to the requested location.

<span>Operation ID</span>`fs.read`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/octet-stream`

<div class="api-schema-node">

<div class="api-schema-heading">

`string<binary>`

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>FileNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[FileNotFoundErrorEncoded](#schema-FileNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/fs/list`<span>List directory</span>

<div class="api-operation-body">

List direct children using an absolute path or a path relative to the requested location, including parents and siblings outside its directory. Entry paths remain relative to the requested location; listing does not switch locations.

<span>Operation ID</span>`fs.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>path</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<p>An absolute path or a path relative to the requested location. Defaults to the location directory.</p>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`FileSystem.Entry[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[FileSystem.Entry](#schema-FileSystem.Entry)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/fs/find`<span>Find files</span>

<div class="api-operation-body">

Find recursively ranked filesystem entries relative to the requested location.

<span>Operation ID</span>`fs.find`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>query</code><span class="api-required">required</span></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>type</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>"file" | "directory"</code>
</div>
<div class="api-schema-values">
<span>Values</span><code>"file" | "directory"</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>limit</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`FileSystem.Entry[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[FileSystem.Entry](#schema-FileSystem.Entry)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/fs/write`<span>Write file</span>

<div class="api-operation-body">

Write the raw request body to an absolute path or a path relative to the requested location, creating parent directories, and return the resolved absolute path. Unlike read, the target is not confined to the location. Experimental: may change without compatibility guarantees.

<span>Operation ID</span>`experimental.fs.write`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>path</code><span class="api-required">required</span></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<p>An absolute path or a path relative to the requested location. Missing parent directories are created.</p>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/octet-stream`

<div class="api-schema-node">

<div class="api-schema-heading">

`string<binary>`

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[FileSystem.Write](#schema-FileSystem.Write)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-command" class="section api-group">

Resource

## command

Experimental command routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/command`<span>List commands</span>

<div class="api-operation-body">

Retrieve currently registered commands.

<span>Operation ID</span>`command.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Command.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Command.Info](#schema-Command.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-skill" class="section api-group">

Resource

## skill

Experimental skill routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/skill`<span>List skills</span>

<div class="api-operation-body">

Retrieve currently registered skills.

<span>Operation ID</span>`skill.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Skill.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Skill.Info](#schema-Skill.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-rpc" class="section api-group">

Resource

## rpc

Plugin RPC routes.

<div class="api-operations">

<span class="api-method api-method-post">post</span>`/api/rpc/{rpcID}/{method}`<span>Call a plugin RPC</span>

<div class="api-operation-body">

Dispatch a method to the currently registered RPC at the requested location.

<span>Operation ID</span>`rpc.call`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>rpcID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>method</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[Rpc.Input](#schema-Rpc.Input)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Rpc.Output</span>

</div>

<div class="api-response-schema">

`application/json`[Rpc.Output](#schema-Rpc.Output)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>RpcError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`RpcErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[RpcErrorEncoded](#schema-RpcErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`500`<span>RpcInternalError</span>

</div>

<div class="api-response-schema">

`application/json`[RpcInternalErrorEncoded](#schema-RpcInternalErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-event" class="section api-group">

Resource

## event

Experimental event stream routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/event`<span>Subscribe to events</span>

<div class="api-operation-body">

Subscribe to native events and plugin RPC events across all server locations. Volatile by contract: a slow consumer overflows and fails the stream, and events during disconnection are missed.

<span>Operation ID</span>`event.subscribe`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`text/event-stream`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`event`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`data`[V2EventEncoded](#schema-V2EventEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-pty" class="section api-group">

Resource

## pty

Experimental location-scoped PTY routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/pty`<span>List PTY sessions</span>

<div class="api-operation-body">

List PTY sessions for a location, including exited sessions retained until removal.

<span>Operation ID</span>`pty.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Pty[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Pty](#schema-Pty)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/pty`<span>Create PTY session</span>

<div class="api-operation-body">

Create a pseudo-terminal session for a location.

<span>Operation ID</span>`pty.create`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`args`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`env`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Pty](#schema-Pty)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/pty/{ptyID}`<span>Get PTY session</span>

<div class="api-operation-body">

Get one PTY session, including its exit code once exited.

<span>Operation ID</span>`pty.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Pty](#schema-Pty)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-put">put</span>`/api/pty/{ptyID}`<span>Update PTY session</span>

<div class="api-operation-body">

Update the title or viewport size of one PTY session.

<span>Operation ID</span>`pty.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`rows`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`cols`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Pty](#schema-Pty)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/pty/{ptyID}`<span>Remove PTY session</span>

<div class="api-operation-body">

Terminate and remove one PTY session.

<span>Operation ID</span>`pty.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/pty/{ptyID}/connect-token`<span>Create PTY WebSocket token</span>

<div class="api-operation-body">

Create a short-lived single-use ticket for opening a PTY WebSocket connection.

<span>Operation ID</span>`pty.connect.token`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>x-opencode-ticket</code></td>
<td>header</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[PtyTicket.ConnectToken](#schema-PtyTicket.ConnectToken)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`403`<span>ForbiddenError</span>

</div>

<div class="api-response-schema">

`application/json`[ForbiddenErrorEncoded](#schema-ForbiddenErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/pty/{ptyID}/connect`<span>Connect to PTY session</span>

<div class="api-operation-body">

Establish a WebSocket connection streaming PTY output and accepting terminal input.

<span>Operation ID</span>`pty.connect`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location[directory]</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>cursor</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>ticket</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`403`<span>ForbiddenError</span>

</div>

<div class="api-response-schema">

`application/json`[ForbiddenErrorEncoded](#schema-ForbiddenErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-persistentpty" class="section api-group">

Resource

## persistentPty

Prototype persistent PTY routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/experimental/session/{sessionID}/terminal/read`<span>Read the session's most recently controlled terminal</span>

<div class="api-operation-body">

Read the last physical rows without changing selection or taking control. Omitted lines uses the live terminal height; larger counts include retained history. Blank rows are preserved. Screen dimensions and cursor remain relative to the live screen. Returns null when no current terminal exists. Selection is server-local and resets on restart. Experimental: may change without compatibility guarantees.

<span>Operation ID</span>`server.experimental.persistentPty.read`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>lines</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>PersistentPty.ReadLinesEncoded | null</code>
</div>
<div class="api-schema-variants">
<div>
<a href="#schema-PersistentPty.ReadLinesEncoded" class="api-schema-link">PersistentPty.ReadLinesEncoded</a>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`PersistentPty.ReadResult | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[PersistentPty.ReadResult](#schema-PersistentPty.ReadResult)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/session/{sessionID}/terminal`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`PersistentPty.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[PersistentPty.Info](#schema-PersistentPty.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/session/{sessionID}/terminal`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.create`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>sessionID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^ses
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[PersistentPty.CreateInput](#schema-PersistentPty.CreateInput)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[PersistentPty.Info](#schema-PersistentPty.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/persistent-pty/shutdown`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.shutdown`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/persistent-pty/handoff`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.handoff`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`handoff`

<div class="api-schema-node">

<div class="api-schema-heading">

`PersistentPty.Handoff | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[PersistentPty.Handoff](#schema-PersistentPty.Handoff)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/persistent-pty/{ptyID}`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[PersistentPty.Info](#schema-PersistentPty.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-put">put</span>`/api/experimental/persistent-pty/{ptyID}`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.update`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[PersistentPty.UpdateInput](#schema-PersistentPty.UpdateInput)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[PersistentPty.Info](#schema-PersistentPty.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/experimental/persistent-pty/{ptyID}`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/persistent-pty/{ptyID}/snapshot`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.snapshot`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[PersistentPty.Snapshot](#schema-PersistentPty.Snapshot)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/experimental/persistent-pty/{ptyID}/connect-token`<span></span>

<div class="api-operation-body">

<span>Operation ID</span>`server.experimental.persistentPty.connectToken`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>x-opencode-ticket</code></td>
<td>header</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[PtyTicket.ConnectToken](#schema-PtyTicket.ConnectToken)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`403`<span>ForbiddenError</span>

</div>

<div class="api-response-schema">

`application/json`[ForbiddenErrorEncoded](#schema-ForbiddenErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/experimental/persistent-pty/{ptyID}/connect`<span>Connect to a persistent PTY</span>

<div class="api-operation-body">

Stream persistent PTY output through the OpenCode server.

<span>Operation ID</span>`persistentPty.connect`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>ptyID</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^pty
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>cursor</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>role</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>attachment_id</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>takeover</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>input_protocol</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>ticket</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`403`<span>ForbiddenError</span>

</div>

<div class="api-response-schema">

`application/json`[ForbiddenErrorEncoded](#schema-ForbiddenErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>PtyNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[PtyNotFoundErrorEncoded](#schema-PtyNotFoundErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-shell" class="section api-group">

Resource

## shell

Location-scoped shell command routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/shell`<span>List running shell commands</span>

<div class="api-operation-body">

List currently running shell commands for a location. Exited commands are not included.

<span>Operation ID</span>`shell.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Shell.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Shell.Info](#schema-Shell.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/shell`<span>Run shell command</span>

<div class="api-operation-body">

Spawn one non-interactive shell command for a location. Combined stdout/stderr is captured to a file pageable via output.

<span>Operation ID</span>`shell.create`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`timeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Shell.Info](#schema-Shell.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/shell/{id}`<span>Get shell command</span>

<div class="api-operation-body">

Get one shell command, including its status and exit code once exited.

<span>Operation ID</span>`shell.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>id</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^sh_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Shell.Info](#schema-Shell.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ShellNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ShellNotFoundErrorEncoded](#schema-ShellNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/shell/{id}`<span>Remove shell command</span>

<div class="api-operation-body">

Terminate and remove one shell command and its retained output.

<span>Operation ID</span>`shell.remove`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>id</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^sh_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/shell/{id}/output`<span>Read shell output</span>

<div class="api-operation-body">

Page through captured combined output by absolute byte cursor.

<span>Operation ID</span>`shell.output`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>id</code><span class="api-required">required</span></td>
<td>path</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^sh_
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>cursor</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^[+-]?\d*\.?\d+(?:[Ee][+-]?\d+)?$
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>limit</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
<div class="api-schema-constraints">
pattern ^[+-]?\d*\.?\d+(?:[Ee][+-]?\d+)?$
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cursor`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`truncated`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ShellNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ShellNotFoundErrorEncoded](#schema-ShellNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-reference" class="section api-group">

Resource

## reference

Location-scoped project references.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/reference`<span>List references</span>

<div class="api-operation-body">

List references available in the requested location.

<span>Operation ID</span>`reference.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Reference.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Reference.Info](#schema-Reference.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-worktree" class="section api-group">

Resource

## worktree

Project-based worktree management routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/worktree`<span>List worktrees</span>

<div class="api-operation-body">

Return the project's saved worktree inventory without loading configuration or running discovery.

<span>Operation ID</span>`worktree.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>projectID</code><span class="api-required">required</span></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Worktree.List</span>

</div>

<div class="api-response-schema">

`application/json`[Worktree.List](#schema-Worktree.List)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ProjectNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ProjectNotFoundErrorEncoded](#schema-ProjectNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/worktree`<span>Create worktree</span>

<div class="api-operation-body">

Load the project's canonical configuration, create a local worktree using its selected strategy, then run the project's setup script.

<span>Operation ID</span>`worktree.create`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[Worktree.CreateInput](#schema-Worktree.CreateInput)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Worktree.Info</span>

</div>

<div class="api-response-schema">

`application/json`[Worktree.Info](#schema-Worktree.Info)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>WorktreeError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`WorktreeErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[WorktreeErrorEncoded](#schema-WorktreeErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ProjectNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ProjectNotFoundErrorEncoded](#schema-ProjectNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/worktree`<span>Remove worktree</span>

<div class="api-operation-body">

Load the project's canonical configuration and remove a saved worktree using its recorded strategy.

<span>Operation ID</span>`worktree.remove`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[Worktree.RemoveInput](#schema-Worktree.RemoveInput)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>WorktreeError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`WorktreeErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[WorktreeErrorEncoded](#schema-WorktreeErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ProjectNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ProjectNotFoundErrorEncoded](#schema-ProjectNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/worktree/refresh`<span>Refresh worktrees</span>

<div class="api-operation-body">

Load the project's canonical configuration, discover worktrees across known checkout roots using all available strategies, and reconcile saved state.

<span>Operation ID</span>`worktree.refresh`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>WorktreeError | InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`WorktreeErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[WorktreeErrorEncoded](#schema-WorktreeErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`404`<span>ProjectNotFoundError</span>

</div>

<div class="api-response-schema">

`application/json`[ProjectNotFoundErrorEncoded](#schema-ProjectNotFoundErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-vcs" class="section api-group">

Resource

## vcs

Location-scoped version control routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/vcs`<span>VCS info</span>

<div class="api-operation-body">

Get current and default branch information for the requested location.

<span>Operation ID</span>`vcs.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Vcs.Info](#schema-Vcs.Info)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/vcs/base`<span>VCS review base</span>

<div class="api-operation-body">

Infer a local review base from named branch creation history, or the repository default only when currently on that branch. Returns null before the first commit or when the provider lacks base metadata; ambiguous Git history requires an explicit base on diff requests.

<span>Operation ID</span>`vcs.base`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Vcs.Base | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[Vcs.Base](#schema-Vcs.Base)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/vcs/status`<span>VCS status</span>

<div class="api-operation-body">

List uncommitted working-copy changes relative to the requested location.

<span>Operation ID</span>`vcs.status`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Vcs.FileStatus[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Vcs.FileStatus](#schema-Vcs.FileStatus)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/vcs/branch`<span>VCS branches</span>

<div class="api-operation-body">

List local and remote branches available at the requested location.

<span>Operation ID</span>`vcs.branch.list`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>search</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>limit</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[Vcs.BranchList](#schema-Vcs.BranchList)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/vcs/diff`<span>VCS diff</span>

<div class="api-operation-body">

Diff HEAD to the working copy (working), the base merge-base to the working copy (branch), or the base merge-base to HEAD (committed). Omitting base preserves repository-default comparison; supplying it overrides the comparison without saving it.

<span>Operation ID</span>`vcs.diff`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>mode</code><span class="api-required">required</span></td>
<td>query</td>
<td><a href="#schema-Vcs.Mode" class="api-schema-link">Vcs.Mode</a></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="odd">
<td><code>base</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
<tr class="even">
<td><code>context</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`FileDiff.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[FileDiff.Info](#schema-FileDiff.Info)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-debug" class="section api-group">

Resource

## debug

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/debug/location`<span>List loaded locations</span>

<div class="api-operation-body">

List locations currently loaded by the server.

<span>Operation ID</span>`debug.location.list`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`Location.PublicRef[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Location.PublicRef](#schema-Location.PublicRef)

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-delete">delete</span>`/api/debug/location`<span>Evict a loaded location</span>

<div class="api-operation-body">

Dispose the requested location's cached services so its next use boots them fresh.

<span>Operation ID</span>`debug.location.evict`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-migration" class="section api-group">

Resource

## migration

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/experimental/migration/v1`<span>Get V1 migration status</span>

<div class="api-operation-body">

Return the progress of the V1 to V2 session history migration.

<span>Operation ID</span>`experimental.migration.v1.status`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"required" | "completed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"required" | "completed"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running"`

</div>

</div>

</div>

<div class="api-schema-property">

`progress`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`numerator`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`denominator`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"error"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"error"`

</div>

</div>

</div>

<div class="api-schema-property">

`error`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-websearch" class="section api-group">

Resource

## websearch

Location-scoped web search routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/websearch/provider`<span>List web search providers</span>

<div class="api-operation-body">

Return the registered web search providers.

<span>Operation ID</span>`websearch.providers`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`WebSearch.Provider[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[WebSearch.Provider](#schema-WebSearch.Provider)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-post">post</span>`/api/websearch`<span>Search the web</span>

<div class="api-operation-body">

Run one web search through the selected provider. Specify a provider to override the configured default.

<span>Operation ID</span>`websearch.query`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`query`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`data`[WebSearch.ResponseEncoded](#schema-WebSearch.ResponseEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`InvalidRequestErrorEncoded | InvalidRequestErrorEncoded`

</div>

<div class="api-schema-variants">

<div>

[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`503`<span>ServiceUnavailableError</span>

</div>

<div class="api-response-schema">

`application/json`[ServiceUnavailableErrorEncoded](#schema-ServiceUnavailableErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="tag-config" class="section api-group">

Resource

## config

Location-scoped configuration routes.

<div class="api-operations">

<span class="api-method api-method-get">get</span>`/api/config`<span>Get configuration</span>

<div class="api-operation-body">

Return configuration documents and discovery sources for the requested location, from lowest to highest priority.

<span>Operation ID</span>`config.get`

<div class="section api-block">

### Parameters

<div class="api-table-scroll">

<table>
<colgroup>
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
<col style="width: 25%" />
</colgroup>
<thead>
<tr class="header">
<th>Name</th>
<th>Location</th>
<th>Type</th>
<th>Description</th>
</tr>
</thead>
<tbody>
<tr class="odd">
<td><code>location</code></td>
<td>query</td>
<td><div class="api-schema-node">
<div class="api-schema-heading">
<code>object | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>object</code>
</div>
<div class="api-schema-properties">
<div class="api-schema-property">
<code>directory</code>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string | null</code>
</div>
<div class="api-schema-variants">
<div>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>string</code>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
</div>
<div>
<span class="api-schema-operator">or</span>
<div class="api-schema-node">
<div class="api-schema-heading">
<code>null</code>
</div>
</div>
</div>
</div>
</div></td>
<td><span class="api-muted">No description</span></td>
</tr>
</tbody>
</table>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`Config.Entry[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Config.Entry](#schema-Config.Entry)

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-get">get</span>`/api/config/shell`<span>List available shells</span>

<div class="api-operation-body">

Return shells available to terminal and agent execution.

<span>Operation ID</span>`config.shells`

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`200`<span>Success</span>

</div>

<div class="api-response-schema">

`application/json`

<div class="api-schema-node">

<div class="api-schema-heading">

`ConfigShell.Option[]`

</div>

<div class="api-schema-items">

<span>Items</span>[ConfigShell.Option](#schema-ConfigShell.Option)

</div>

</div>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

<span class="api-method api-method-patch">patch</span>`/api/experimental/config`<span>Update global configuration</span>

<div class="api-operation-body">

Patch supported fields in the highest-precedence global configuration document.

<span>Operation ID</span>`experimental.config.update`

<div class="section api-block">

### Request body <span class="api-required">required</span>

<div class="api-content-schemas">

<div>

`application/json`[Config.Patch](#schema-Config.Patch)

</div>

</div>

</div>

<div class="section api-block">

### Responses

<div class="api-responses">

<div class="api-response">

<div class="api-response-heading">

`204`<span>\<No Content\></span>

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`400`<span>InvalidRequestError</span>

</div>

<div class="api-response-schema">

`application/json`[InvalidRequestErrorEncoded](#schema-InvalidRequestErrorEncoded)

</div>

</div>

<div class="api-response">

<div class="api-response-heading">

`401`<span>UnauthorizedError</span>

</div>

<div class="api-response-schema">

`application/json`[UnauthorizedErrorEncoded](#schema-UnauthorizedErrorEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div id="schemas" class="section api-group api-schemas">

Components

## Schemas

Reusable data types referenced by API operations.

<div class="api-operations">

`Agent.Color`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

`Agent.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

<div class="api-schema-property">

`request`[Provider.Request](#schema-Provider.Request)

</div>

<div class="api-schema-property">

`system`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mode`

<div class="api-schema-node">

<div class="api-schema-heading">

`"subagent" | "primary" | "all"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"subagent" | "primary" | "all"`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`color`[Agent.Color](#schema-Agent.Color)

</div>

<div class="api-schema-property">

`steps`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`permissions`[Permission.Ruleset](#schema-Permission.Ruleset)

</div>

</div>

</div>

</div>

`AgentNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"AgentNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"AgentNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`agentID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Command.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`CommandExecutionErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"CommandExecutionError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"CommandExecutionError"`

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`CommandNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"CommandNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"CommandNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.AgentEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[^/\#\]+\\/\[^\#\]+(?:\#\[^\#\]+)?$

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^\[^/\#\]+$

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^\[^\#\]+$

</div>

</div>

</div>

<div class="api-schema-property">

`variant`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[^\#\]+$

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`request`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`system`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mode`

<div class="api-schema-node">

<div class="api-schema-heading">

`"subagent" | "primary" | "all"`

</div>

<div class="api-schema-values">

<span>Values</span>`"subagent" | "primary" | "all"`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`color`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\#\[0-9a-fA-F\]{6}$

</div>

</div>

</div>

<div class="api-schema-property">

`steps`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`permissions`[Permission.Ruleset](#schema-Permission.Ruleset)

</div>

</div>

</div>

</div>

`Config.CommandEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`template`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[^/\#\]+\\/\[^\#\]+(?:\#\[^\#\]+)?$

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^\[^/\#\]+$

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^\[^\#\]+$

</div>

</div>

</div>

<div class="api-schema-property">

`variant`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[^\#\]+$

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`subagent`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`subtask`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

Deprecated alias for subagent.

</div>

</div>

</div>

</div>

</div>

`Config.DirectoryEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"directory"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"directory"`

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.DocumentEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"document"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"document"`

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`info`[Config.InfoEncoded](#schema-Config.InfoEncoded)

</div>

</div>

</div>

</div>

`Config.Entry`<span>Config.DocumentEncoded | Config.DirectoryEncoded</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Config.DocumentEncoded | Config.DirectoryEncoded`

</div>

<div class="api-schema-variants">

<div>

[Config.DocumentEncoded](#schema-Config.DocumentEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[Config.DirectoryEncoded](#schema-Config.DirectoryEncoded)

</div>

</div>

</div>

</div>

`Config.Formatter.EntryEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`environment`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`extensions`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.InfoEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`$schema`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`shell`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[^/\#\]+\\/\[^\#\]+(?:\#\[^\#\]+)?$

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^\[^/\#\]+$

</div>

</div>

</div>

<div class="api-schema-property">

`model`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^\[^\#\]+$

</div>

</div>

</div>

<div class="api-schema-property">

`variant`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[^\#\]+$

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`default_agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`update`

<div class="api-schema-node">

<div class="api-schema-heading">

`"disable" | "notify" | "auto"`

</div>

<div class="api-schema-values">

<span>Values</span>`"disable" | "notify" | "auto"`

</div>

</div>

</div>

<div class="api-schema-property">

`share`

<div class="api-schema-node">

<div class="api-schema-heading">

`"manual" | "auto" | "disabled"`

</div>

<div class="api-schema-values">

<span>Values</span>`"manual" | "auto" | "disabled"`

</div>

</div>

</div>

<div class="api-schema-property">

`enterprise`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`username`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`permissions`[Permission.Ruleset](#schema-Permission.Ruleset)

</div>

<div class="api-schema-property">

`agents`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>[Config.AgentEncoded](#schema-Config.AgentEncoded)

</div>

</div>

</div>

<div class="api-schema-property">

`snapshots`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`watcher`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`ignore`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`formatter`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>[Config.Formatter.EntryEncoded](#schema-Config.Formatter.EntryEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`lsp`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object | Config.LSP.ServerEncoded`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`true`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`true`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>[Config.LSP.ServerEncoded](#schema-Config.LSP.ServerEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`media`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`image`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`auto_resize`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`max_width`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`max_height`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`max_base64_bytes`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`tool_output`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`max_lines`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`max_bytes`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`mcp`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`timeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`startup`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`catalog`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`execution`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`servers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.LocalConfigEncoded | Mcp.RemoteConfigEncoded`

</div>

<div class="api-schema-variants">

<div>

[Mcp.LocalConfigEncoded](#schema-Mcp.LocalConfigEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[Mcp.RemoteConfigEncoded](#schema-Mcp.RemoteConfigEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`compaction`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`auto`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`keep`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`tokens`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`buffer`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`skills`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`commands`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>[Config.CommandEncoded](#schema-Config.CommandEncoded)

</div>

</div>

</div>

<div class="api-schema-property">

`instructions`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`references`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string | Config.Reference.GitEncoded | Config.Reference.LocalEncoded`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>[Config.Reference.GitEncoded](#schema-Config.Reference.GitEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>[Config.Reference.LocalEncoded](#schema-Config.Reference.LocalEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`websearch`

<div class="api-schema-node">

<div class="api-schema-heading">

`false | ConfigWebSearch.InfoEncoded`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`false`

</div>

<div class="api-schema-values">

<span>Values</span>`false`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>[ConfigWebSearch.InfoEncoded](#schema-ConfigWebSearch.InfoEncoded)

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`plugins`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | Config.Plugin.EntryEncoded[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string | Config.Plugin.EntryEncoded`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>[Config.Plugin.EntryEncoded](#schema-Config.Plugin.EntryEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`worktree`[Config.Worktree](#schema-Config.Worktree)

</div>

<div class="api-schema-property">

`warming`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | Config.WarmingEncoded`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>[Config.WarmingEncoded](#schema-Config.WarmingEncoded)

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`providers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>[Config.ProviderEncoded](#schema-Config.ProviderEncoded)

</div>

</div>

</div>

<div class="api-schema-property">

`experimental`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`portable_shell_scanner`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`subagent_depth`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`policies`

<div class="api-schema-node">

<div class="api-schema-heading">

`object[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`action`

<div class="api-schema-node">

<div class="api-schema-heading">

`"provider.use" | "permission"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"provider.use" | "permission"`

</div>

</div>

</div>

<div class="api-schema-property">

`resource`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`effect`

<div class="api-schema-node">

<div class="api-schema-heading">

`"allow" | "deny"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"allow" | "deny"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.LSP.ServerEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`extensions`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`env`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`initialization`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Model.CostEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`tier`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"context"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"context"`

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`input`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

<div class="api-schema-property">

`output`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

<div class="api-schema-property">

`cache`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`read`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

<div class="api-schema-property">

`write`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Model.Settings`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`compaction`[Provider.Compaction](#schema-Provider.Compaction)

</div>

</div>

</div>

</div>

`Config.ModelEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`modelID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`family`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`compatibility`[Model.Compatibility](#schema-Model.Compatibility)

</div>

<div class="api-schema-property">

`package`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`settings`[Config.Model.Settings](#schema-Config.Model.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`capabilities`[Model.Capabilities](#schema-Model.Capabilities)

</div>

<div class="api-schema-property">

`variants`

<div class="api-schema-node">

<div class="api-schema-heading">

`object[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`settings`[Config.Model.Settings](#schema-Config.Model.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cost`

<div class="api-schema-node">

<div class="api-schema-heading">

`Config.Model.CostEncoded | Config.Model.CostEncoded[]`

</div>

<div class="api-schema-variants">

<div>

[Config.Model.CostEncoded](#schema-Config.Model.CostEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`Config.Model.CostEncoded[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Config.Model.CostEncoded](#schema-Config.Model.CostEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`limit`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`context`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

</div>

</div>

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Patch`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`shell`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Plugin.EntryEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`package`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`options`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Provider.Settings`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`timeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | false`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`false`

</div>

<div class="api-schema-values">

<span>Values</span>`false`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`chunkTimeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`compaction`[Provider.Compaction](#schema-Provider.Compaction)

</div>

<div class="api-schema-property">

`transport`[Provider.Transport](#schema-Provider.Transport)

</div>

</div>

</div>

</div>

`Config.ProviderEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`canonical`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`env`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`package`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`settings`[Config.Provider.Settings](#schema-Config.Provider.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`models`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>[Config.ModelEncoded](#schema-Config.ModelEncoded)

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Reference.GitEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`repository`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`branch`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Reference.LocalEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

</div>

</div>

`Config.WarmingEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`prompt`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`interval`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`duration`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Config.Worktree`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`ConfigShell.Option`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`acceptable`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`ConfigWebSearch.InfoEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`provider`

<div class="api-schema-node">

<div class="api-schema-heading">

`"random" | string`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`"random"`

</div>

Reuse a randomly selected provider until it is rate limited, then switch to another available provider.

<div class="api-schema-values">

<span>Values</span>`"random"`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`ConflictErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"ConflictError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"ConflictError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`resource`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Connection.CredentialInfo`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"credential"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"credential"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`method`

<div class="api-schema-node">

<div class="api-schema-heading">

`"key" | "oauth"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"key" | "oauth"`

</div>

</div>

</div>

</div>

</div>

</div>

`Connection.EnvInfo`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"env"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"env"`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Connection.Info`<span>Connection.CredentialInfo | Connection.EnvInfo</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Connection.CredentialInfo | Connection.EnvInfo`

</div>

<div class="api-schema-variants">

<div>

[Connection.CredentialInfo](#schema-Connection.CredentialInfo)

</div>

<div>

<span class="api-schema-operator">or</span>[Connection.EnvInfo](#schema-Connection.EnvInfo)

</div>

</div>

</div>

</div>

`FileDiff.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`file`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`patch`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`additions`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`deletions`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"added" | "deleted" | "modified"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"added" | "deleted" | "modified"`

</div>

</div>

</div>

</div>

</div>

</div>

`FileNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"FileNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"FileNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`FileSystem.Entry`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"file" | "directory"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"file" | "directory"`

</div>

</div>

</div>

</div>

</div>

</div>

`FileSystem.Write`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`ForbiddenErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"ForbiddenError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"ForbiddenError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Form.Answer`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>[Form.Value](#schema-Form.Value)

</div>

</div>

</div>

`Form.BooleanField`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`required`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`when`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.When[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Form.When](#schema-Form.When)

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"boolean"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"boolean"`

</div>

</div>

</div>

<div class="api-schema-property">

`default`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

</div>

</div>

`Form.CreatePayload`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^frm\_

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`[Form.Metadata](#schema-Form.Metadata)

</div>

<div class="api-schema-property">

`fields`[Form.Fields](#schema-Form.Fields)

</div>

</div>

</div>

</div>

`Form.Detail`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^frm\_

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`[Form.Metadata](#schema-Form.Metadata)

</div>

<div class="api-schema-property">

`fields`[Form.Fields](#schema-Form.Fields)

</div>

<div class="api-schema-property">

`state`[Form.State](#schema-Form.State)

</div>

</div>

</div>

</div>

`Form.ExternalField`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"external"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"external"`

</div>

</div>

</div>

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Form.Field`<span>Form.StringField | Form.NumberField | Form.IntegerField | Form.BooleanField | Form.MultiselectField | Form.ExternalField</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.StringField | Form.NumberField | Form.IntegerField | Form.BooleanField | Form.MultiselectField | Form.ExternalField`

</div>

<div class="api-schema-variants">

<div>

[Form.StringField](#schema-Form.StringField)

</div>

<div>

<span class="api-schema-operator">or</span>[Form.NumberField](#schema-Form.NumberField)

</div>

<div>

<span class="api-schema-operator">or</span>[Form.IntegerField](#schema-Form.IntegerField)

</div>

<div>

<span class="api-schema-operator">or</span>[Form.BooleanField](#schema-Form.BooleanField)

</div>

<div>

<span class="api-schema-operator">or</span>[Form.MultiselectField](#schema-Form.MultiselectField)

</div>

<div>

<span class="api-schema-operator">or</span>[Form.ExternalField](#schema-Form.ExternalField)

</div>

</div>

</div>

</div>

`Form.Fields`<span>Form.Field\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Field[]`

</div>

<div class="api-schema-constraints">

min items 1

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Field](#schema-Form.Field)

</div>

</div>

</div>

`Form.Fields_1`<span>Form.Field\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Field[]`

</div>

<div class="api-schema-constraints">

min items 1

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Field](#schema-Form.Field)

</div>

</div>

</div>

`Form.Fields_2`<span>Form.Field\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Field[]`

</div>

<div class="api-schema-constraints">

min items 1

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Field](#schema-Form.Field)

</div>

</div>

</div>

`Form.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^frm\_

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`[Form.Metadata](#schema-Form.Metadata)

</div>

<div class="api-schema-property">

`fields`[Form.Fields](#schema-Form.Fields)

</div>

</div>

</div>

</div>

`Form.IntegerField`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`required`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`when`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.When[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Form.When](#schema-Form.When)

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"integer"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"integer"`

</div>

</div>

</div>

<div class="api-schema-property">

`minimum`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`maximum`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`default`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Form.Metadata`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Form.MultiselectField`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`required`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`when`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.When[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Form.When](#schema-Form.When)

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"multiselect"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"multiselect"`

</div>

</div>

</div>

<div class="api-schema-property">

`options`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Option[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Option](#schema-Form.Option)

</div>

</div>

</div>

<div class="api-schema-property">

`minItems`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`maxItems`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`custom`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`default`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Form.NumberField`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`required`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`when`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.When[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Form.When](#schema-Form.When)

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"number"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"number"`

</div>

</div>

</div>

<div class="api-schema-property">

`minimum`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`maximum`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`default`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Form.Option`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`value`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Form.Reply`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`answer`[Form.Answer](#schema-Form.Answer)

</div>

</div>

</div>

</div>

`Form.State`<span>object | object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"pending"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"pending"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"answered"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"answered"`

</div>

</div>

</div>

<div class="api-schema-property">

`answer`[Form.Answer](#schema-Form.Answer)

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"cancelled"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"cancelled"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Form.StringField`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`required`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`when`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.When[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Form.When](#schema-Form.When)

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"string"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"string"`

</div>

</div>

</div>

<div class="api-schema-property">

`format`

<div class="api-schema-node">

<div class="api-schema-heading">

`"email" | "uri" | "date" | "date-time"`

</div>

<div class="api-schema-values">

<span>Values</span>`"email" | "uri" | "date" | "date-time"`

</div>

</div>

</div>

<div class="api-schema-property">

`minLength`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`maxLength`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`pattern`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`placeholder`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`default`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`options`

<div class="api-schema-node">

<div class="api-schema-heading">

`Form.Option[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Form.Option](#schema-Form.Option)

</div>

</div>

</div>

<div class="api-schema-property">

`custom`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

</div>

</div>

`Form.Value`<span>string | number | "Infinity" | "-Infinity" | "NaN" | boolean | string\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string | number | "Infinity" | "-Infinity" | "NaN" | boolean | string[]`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Form.When`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`op`

<div class="api-schema-node">

<div class="api-schema-heading">

`"eq" | "neq"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"eq" | "neq"`

</div>

</div>

</div>

<div class="api-schema-property">

`value`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | number | "Infinity" | "-Infinity" | "NaN" | boolean`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`FormAlreadySettledErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"FormAlreadySettledError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"FormAlreadySettledError"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`FormInvalidAnswerErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"FormInvalidAnswerError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"FormInvalidAnswerError"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`FormNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"FormNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"FormNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`GenerateTextResponse`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`InstructionEntry.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`key`[InstructionEntry.Key](#schema-InstructionEntry.Key)

</div>

<div class="api-schema-property">

`value`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

JSON value attached to the session's instructions

</div>

</div>

</div>

</div>

</div>

`InstructionEntry.Key`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

Instruction entry key (lowercase alphanumerics plus . \_ -)

<div class="api-schema-constraints">

pattern ^\[a-z0-9\]\[a-z0-9.\_-\]\*$

</div>

</div>

</div>

`InstructionEntryValueTooLargeErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"InstructionEntryValueTooLargeError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"InstructionEntryValueTooLargeError"`

</div>

</div>

</div>

<div class="api-schema-property">

`actualBytes`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`maxBytes`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.AttemptEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`attemptID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`instructions`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`mode`

<div class="api-schema-node">

<div class="api-schema-heading">

`"auto" | "code"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"auto" | "code"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.AttemptStatus`<span>object | object | object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object | object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"pending"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"pending"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"complete"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"complete"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"failed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"failed"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"expired"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"expired"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.CommandAttempt`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`attemptID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.CommandAttemptStatus`<span>object | object | object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object | object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"pending"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"pending"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"complete"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"complete"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"failed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"failed"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"expired"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"expired"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`expires`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.CommandMethod`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"command"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"command"`

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.EnvMethod`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"env"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"env"`

</div>

</div>

</div>

<div class="api-schema-property">

`names`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`methods`

<div class="api-schema-node">

<div class="api-schema-heading">

`Integration.Method[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Integration.Method](#schema-Integration.Method)

</div>

</div>

</div>

<div class="api-schema-property">

`connections`

<div class="api-schema-node">

<div class="api-schema-heading">

`Connection.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Connection.Info](#schema-Connection.Info)

</div>

</div>

</div>

</div>

</div>

</div>

`Integration.KeyMethod`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"key"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"key"`

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`form`[Form.Fields\_2](#schema-Form.Fields_2)

</div>

</div>

</div>

</div>

`Integration.Method`<span>Integration.OAuthMethod | Integration.CommandMethod | Integration.KeyMethod | Integration.EnvMethod</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Integration.OAuthMethod | Integration.CommandMethod | Integration.KeyMethod | Integration.EnvMethod`

</div>

<div class="api-schema-variants">

<div>

[Integration.OAuthMethod](#schema-Integration.OAuthMethod)

</div>

<div>

<span class="api-schema-operator">or</span>[Integration.CommandMethod](#schema-Integration.CommandMethod)

</div>

<div>

<span class="api-schema-operator">or</span>[Integration.KeyMethod](#schema-Integration.KeyMethod)

</div>

<div>

<span class="api-schema-operator">or</span>[Integration.EnvMethod](#schema-Integration.EnvMethod)

</div>

</div>

</div>

</div>

`Integration.OAuthMethod`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"oauth"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"oauth"`

</div>

</div>

</div>

<div class="api-schema-property">

`label`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`form`[Form.Fields\_1](#schema-Form.Fields_1)

</div>

</div>

</div>

</div>

`IntegrationAttemptNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"IntegrationAttemptNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"IntegrationAttemptNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`integrationID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`attemptID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`IntegrationMethodNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"IntegrationMethodNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"IntegrationMethodNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`integrationID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`methodID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`IntegrationNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"IntegrationNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"IntegrationNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`integrationID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`InvalidCursorErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"InvalidCursorError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"InvalidCursorError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`InvalidRequestErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"InvalidRequestError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"InvalidRequestError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`kind`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`field`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Location.PublicInfo`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`project`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`canonical`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Location.PublicRef`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.LocalConfigEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"local"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"local"`

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`environment`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`codemode`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`timeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`startup`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`catalog`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`execution`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`protocol`[Mcp.Protocol](#schema-Mcp.Protocol)

</div>

</div>

</div>

</div>

`Mcp.OAuthConfigEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`client_id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`client_secret`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`scope`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`callback_port`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 1, max 65535

</div>

</div>

</div>

<div class="api-schema-property">

`redirect_uri`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`auth_server_metadata_url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Protocol`<span>"legacy" | "auto" | "2026-07-28"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"legacy" | "auto" | "2026-07-28"`

</div>

MCP protocol negotiation. "legacy" (default) opens with the initialize handshake and speaks protocol revisions up to 2025-11-25. "auto" probes for the 2026-07-28 revision and falls back to legacy when the server does not support it. "2026-07-28" requires that revision and fails otherwise.

<div class="api-schema-values">

<span>Values</span>`"legacy" | "auto" | "2026-07-28"`

</div>

</div>

</div>

`Mcp.RemoteConfigEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"remote"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"remote"`

</div>

</div>

</div>

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`oauth`

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.OAuthConfigEncoded | false`

</div>

<div class="api-schema-variants">

<div>

[Mcp.OAuthConfigEncoded](#schema-Mcp.OAuthConfigEncoded)

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`false`

</div>

<div class="api-schema-values">

<span>Values</span>`false`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`disabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`codemode`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`timeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`startup`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`catalog`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`execution`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`protocol`[Mcp.Protocol](#schema-Mcp.Protocol)

</div>

</div>

</div>

</div>

`Mcp.Resource`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`server`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`uri`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mimeType`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.ResourceCatalog`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`resources`

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.Resource[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Mcp.Resource](#schema-Mcp.Resource)

</div>

</div>

</div>

<div class="api-schema-property">

`templates`

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.ResourceTemplate[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Mcp.ResourceTemplate](#schema-Mcp.ResourceTemplate)

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.ResourceTemplate`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`server`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`uriTemplate`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mimeType`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Server`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`Mcp.Status.Connected | Mcp.Status.Pending | Mcp.Status.Disabled | Mcp.Status.Failed | Mcp.Status.NeedsAuth`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[Mcp.Status.Connected](#schema-Mcp.Status.Connected)

</div>

<div>

<span class="api-schema-operator">or</span>[Mcp.Status.Pending](#schema-Mcp.Status.Pending)

</div>

<div>

<span class="api-schema-operator">or</span>[Mcp.Status.Disabled](#schema-Mcp.Status.Disabled)

</div>

<div>

<span class="api-schema-operator">or</span>[Mcp.Status.Failed](#schema-Mcp.Status.Failed)

</div>

<div>

<span class="api-schema-operator">or</span>[Mcp.Status.NeedsAuth](#schema-Mcp.Status.NeedsAuth)

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`integrationID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Status.Connected`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"connected"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"connected"`

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Status.Disabled`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"disabled"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"disabled"`

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Status.Failed`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"failed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"failed"`

</div>

</div>

</div>

<div class="api-schema-property">

`error`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Status.NeedsAuth`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"needs_auth"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"needs_auth"`

</div>

</div>

</div>

<div class="api-schema-property">

`error`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Mcp.Status.Pending`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"pending"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"pending"`

</div>

</div>

</div>

</div>

</div>

</div>

`McpServerNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"McpServerNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"McpServerNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`server`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`MessageNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"MessageNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"MessageNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`messageID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Model.Capabilities`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`tools`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Model.Compatibility`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`reasoningField`[Model.ReasoningField](#schema-Model.ReasoningField)

</div>

<div class="api-schema-property">

`requireReasoning`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`maxTokensField`[Model.MaxTokensField](#schema-Model.MaxTokensField)

</div>

<div class="api-schema-property">

`requireFinishReason`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`requireAssistantAfterTool`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`supportsPromptCacheKey`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

</div>

</div>

</div>

`Model.Cost`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`tier`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"context"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"context"`

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`input`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

<div class="api-schema-property">

`output`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

<div class="api-schema-property">

`cache`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`read`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

<div class="api-schema-property">

`write`[Money.USDPerMillionTokens](#schema-Money.USDPerMillionTokens)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Model.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`modelID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`canonical`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`family`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`compatibility`[Model.Compatibility](#schema-Model.Compatibility)

</div>

<div class="api-schema-property">

`package`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`settings`[Model.Settings](#schema-Model.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`capabilities`[Model.Capabilities](#schema-Model.Capabilities)

</div>

<div class="api-schema-property">

`variants`

<div class="api-schema-node">

<div class="api-schema-heading">

`Model.Variant[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Model.Variant](#schema-Model.Variant)

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`released`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cost`

<div class="api-schema-node">

<div class="api-schema-heading">

`Model.Cost[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Model.Cost](#schema-Model.Cost)

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"alpha" | "beta" | "deprecated" | "active"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"alpha" | "beta" | "deprecated" | "active"`

</div>

</div>

</div>

<div class="api-schema-property">

`enabled`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`limit`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`context`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

</div>

</div>

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Model.MaxTokensField`<span>"max\_completion\_tokens" | "max\_tokens"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"max_completion_tokens" | "max_tokens"`

</div>

<div class="api-schema-values">

<span>Values</span>`"max_completion_tokens" | "max_tokens"`

</div>

</div>

</div>

`Model.ReasoningField`<span>"reasoning" | "reasoning\_content" | "reasoning\_text" | string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"reasoning" | "reasoning_content" | "reasoning_text" | string`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`"reasoning" | "reasoning_content" | "reasoning_text"`

</div>

<div class="api-schema-values">

<span>Values</span>`"reasoning" | "reasoning_content" | "reasoning_text"`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Model.Ref`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`variant`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Model.Settings`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`compaction`[Provider.Compaction](#schema-Provider.Compaction)

</div>

</div>

</div>

</div>

`Model.Variant`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`settings`[Model.Settings](#schema-Model.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Money.USD`<span>number</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

`Money.USDPerMillionTokens`<span>number</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

`Permission.Effect`<span>"allow" | "deny" | "ask"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"allow" | "deny" | "ask"`

</div>

<div class="api-schema-values">

<span>Values</span>`"allow" | "deny" | "ask"`

</div>

</div>

</div>

`Permission.Reply`<span>"once" | "always" | "reject"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"once" | "always" | "reject"`

</div>

<div class="api-schema-values">

<span>Values</span>`"once" | "always" | "reject"`

</div>

</div>

</div>

`Permission.Request`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^per

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`action`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`resources`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`save`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`source`[Permission.Source](#schema-Permission.Source)

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Permission.Rule`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`action`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`resource`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`effect`[Permission.Effect](#schema-Permission.Effect)

</div>

</div>

</div>

</div>

`Permission.Ruleset`<span>Permission.Rule\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Permission.Rule[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Permission.Rule](#schema-Permission.Rule)

</div>

</div>

</div>

`Permission.Source`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"tool"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"tool"`

</div>

</div>

</div>

<div class="api-schema-property">

`messageID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PermissionNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"PermissionNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"PermissionNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`requestID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`PermissionSaved.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`action`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`resource`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`updated`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PersistentPty.CreateInput`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`args`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`env`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`cols`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`rows`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PersistentPty.Handoff`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`instanceID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`ticket`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`expiresAt`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PersistentPty.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^pty

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`args`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running" | "exited"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running" | "exited"`

</div>

</div>

</div>

<div class="api-schema-property">

`pid`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`exitCode`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`foregroundProcess`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`cols`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`rows`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`head`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`tail`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PersistentPty.ReadLinesEncoded`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

`PersistentPty.ReadResult`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`ptyID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^pty

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`foregroundProcess`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`screen`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cols`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`rows`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`cursor`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`x`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`y`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PersistentPty.Snapshot`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`info`[PersistentPty.Info](#schema-PersistentPty.Info)

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`checkpoint`

<div class="api-schema-node">

<div class="api-schema-heading">

`string<byte>`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cursor`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`x`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`y`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`PersistentPty.UpdateInput`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`attachmentID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`cols`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`rows`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Plugin.Features`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`server`

<div class="api-schema-node">

<div class="api-schema-heading">

`true`

</div>

<div class="api-schema-values">

<span>Values</span>`true`

</div>

</div>

</div>

<div class="api-schema-property">

`tui`

<div class="api-schema-node">

<div class="api-schema-heading">

`true`

</div>

<div class="api-schema-values">

<span>Values</span>`true`

</div>

</div>

</div>

<div class="api-schema-property">

`rpc`

<div class="api-schema-node">

<div class="api-schema-heading">

`true`

</div>

<div class="api-schema-values">

<span>Values</span>`true`

</div>

</div>

</div>

</div>

</div>

</div>

`Plugin.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`source`[Plugin.Source](#schema-Plugin.Source)

</div>

<div class="api-schema-property">

`features`[Plugin.Features](#schema-Plugin.Features)

</div>

<div class="api-schema-property">

`state`[Plugin.State](#schema-Plugin.State)

</div>

</div>

</div>

</div>

`Plugin.Source`<span>object | object | object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object | object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"builtin"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"builtin"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"package"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"package"`

</div>

</div>

</div>

<div class="api-schema-property">

`target`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`version`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`outdated`

<div class="api-schema-node">

<div class="api-schema-heading">

`true`

</div>

<div class="api-schema-values">

<span>Values</span>`true`

</div>

</div>

</div>

<div class="api-schema-property">

`updating`

<div class="api-schema-node">

<div class="api-schema-heading">

`true`

</div>

<div class="api-schema-values">

<span>Values</span>`true`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"local"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"local"`

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"sdk"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"sdk"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Plugin.State`<span>object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"active"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"active"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"failed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"failed"`

</div>

</div>

</div>

<div class="api-schema-property">

`error`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`ref`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Project`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`canonical`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`vcs`[Project.Vcs](#schema-Project.Vcs)

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`icon`[Project.Icon](#schema-Project.Icon)

</div>

<div class="api-schema-property">

`commands`[Project.Commands](#schema-Project.Commands)

</div>

<div class="api-schema-property">

`time`[Project.Time](#schema-Project.Time)

</div>

<div class="api-schema-property">

`sandboxes`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Project.Commands`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`start`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

Startup script to run when creating a new workspace (worktree)

</div>

</div>

</div>

</div>

</div>

`Project.Icon`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`override`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`color`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Project.Time`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`updated`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`active`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

`Project.Vcs`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^\[a-z\]\[a-z0-9.\_-\]\*$

</div>

</div>

</div>

`ProjectNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"ProjectNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"ProjectNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Prompt.AgentAttachment`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`mention`[Prompt.Mention](#schema-Prompt.Mention)

</div>

</div>

</div>

</div>

`Prompt.Base64`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^(?:\[A-Za-z0-9+/\]{4})\*(?:\[A-Za-z0-9+/\]{2}==|\[A-Za-z0-9+/\]{3}=)?$

</div>

</div>

</div>

`Prompt.FileAttachment`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`[Prompt.Base64](#schema-Prompt.Base64)

</div>

<div class="api-schema-property">

`mime`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`source`[Prompt.FileSource](#schema-Prompt.FileSource)

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mention`[Prompt.Mention](#schema-Prompt.Mention)

</div>

</div>

</div>

</div>

`Prompt.FileSource`<span>object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"inline"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"inline"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"uri"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"uri"`

</div>

</div>

</div>

<div class="api-schema-property">

`uri`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Prompt.Mention`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`start`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`end`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Prompt.SkillAttachment`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mention`[Prompt.Mention](#schema-Prompt.Mention)

</div>

</div>

</div>

</div>

`PromptInput.FileAttachment`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`uri`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`mention`[Prompt.Mention](#schema-Prompt.Mention)

</div>

</div>

</div>

</div>

`PromptInput.SkillAttachment`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`mention`[Prompt.Mention](#schema-Prompt.Mention)

</div>

</div>

</div>

</div>

`Provider.Compaction`<span>object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"summary"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"summary"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"native"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"native"`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Provider.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`canonical`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`integrationID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`activation`

<div class="api-schema-node">

<div class="api-schema-heading">

`"auto" | "enabled" | "disabled"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"auto" | "enabled" | "disabled"`

</div>

</div>

</div>

<div class="api-schema-property">

`package`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`settings`[Provider.Settings](#schema-Provider.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Provider.Request`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`settings`[Provider.Settings](#schema-Provider.Settings)

</div>

<div class="api-schema-property">

`headers`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`body`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Provider.Settings`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-items">

<span>Additional properties</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`timeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | false`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`false`

</div>

<div class="api-schema-values">

<span>Values</span>`false`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`chunkTimeout`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`compaction`[Provider.Compaction](#schema-Provider.Compaction)

</div>

<div class="api-schema-property">

`transport`[Provider.Transport](#schema-Provider.Transport)

</div>

</div>

</div>

</div>

`Provider.Transport`<span>"http" | "websocket"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"http" | "websocket"`

</div>

<div class="api-schema-values">

<span>Values</span>`"http" | "websocket"`

</div>

</div>

</div>

`ProviderNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"ProviderNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"ProviderNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Pty`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^pty

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`args`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running" | "exited"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running" | "exited"`

</div>

</div>

</div>

<div class="api-schema-property">

`pid`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`exitCode`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

`PtyNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"PtyNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"PtyNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`ptyID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`PtyTicket.ConnectToken`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`ticket`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`expires_in`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

</div>

</div>

</div>

`Reference.GitSource`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"git"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"git"`

</div>

</div>

</div>

<div class="api-schema-property">

`repository`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`branch`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Reference.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`hidden`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`source`[Reference.Source](#schema-Reference.Source)

</div>

</div>

</div>

</div>

`Reference.LocalSource`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"local"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"local"`

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Reference.Source`<span>Reference.LocalSource | Reference.GitSource</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Reference.LocalSource | Reference.GitSource`

</div>

<div class="api-schema-variants">

<div>

[Reference.LocalSource](#schema-Reference.LocalSource)

</div>

<div>

<span class="api-schema-operator">or</span>[Reference.GitSource](#schema-Reference.GitSource)

</div>

</div>

</div>

</div>

`Rpc.Input`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Rpc.Output`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`RpcErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"RpcError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"RpcError"`

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`RpcInternalErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"RpcInternalError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"RpcInternalError"`

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"rpc.internal" | "rpc.invalid_output"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"rpc.internal" | "rpc.invalid_output"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`ServerInfo`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`version`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`pid`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`urls`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`paths`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`tmp`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`ServiceUnavailableErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"ServiceUnavailableError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"ServiceUnavailableError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`service`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.ForkBoundary`<span>object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"before"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"before"`

</div>

</div>

</div>

<div class="api-schema-property">

`messageID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"through"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"through"`

</div>

</div>

</div>

<div class="api-schema-property">

`messageID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Inbox.Compaction`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"compaction"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"compaction"`

</div>

</div>

</div>

<div class="api-schema-property">

`payload`[Session.Inbox.CompactionPayload](#schema-Session.Inbox.CompactionPayload)

</div>

<div class="api-schema-property">

`delivery`[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

</div>

</div>

</div>

`Session.Inbox.CompactionPayload`<span>object | empty\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | empty[]`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`empty[]`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Inbox.Delivery`<span>"steer" | "queue"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"steer" | "queue"`

</div>

<div class="api-schema-values">

<span>Values</span>`"steer" | "queue"`

</div>

</div>

</div>

`Session.Inbox.Info`<span>Session.Inbox.User | Session.Inbox.Synthetic | Session.Inbox.Compaction | Session.Inbox.Move</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Inbox.User | Session.Inbox.Synthetic | Session.Inbox.Compaction | Session.Inbox.Move`

</div>

<div class="api-schema-variants">

<div>

[Session.Inbox.User](#schema-Session.Inbox.User)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Inbox.Synthetic](#schema-Session.Inbox.Synthetic)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Inbox.Compaction](#schema-Session.Inbox.Compaction)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Inbox.Move](#schema-Session.Inbox.Move)

</div>

</div>

</div>

</div>

`Session.Inbox.Move`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"move"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"move"`

</div>

</div>

</div>

<div class="api-schema-property">

`delivery`[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

<div class="api-schema-property">

`payload`[Session.Inbox.MovePayload](#schema-Session.Inbox.MovePayload)

</div>

</div>

</div>

</div>

`Session.Inbox.MovePayload`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`subpath`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

</div>

</div>

</div>

`Session.Inbox.Synthetic`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"synthetic"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"synthetic"`

</div>

</div>

</div>

<div class="api-schema-property">

`payload`[Session.Inbox.SyntheticPayload](#schema-Session.Inbox.SyntheticPayload)

</div>

<div class="api-schema-property">

`delivery`[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

</div>

</div>

</div>

`Session.Inbox.SyntheticPayload`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Inbox.User`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"user"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"user"`

</div>

</div>

</div>

<div class="api-schema-property">

`payload`[Session.Inbox.UserPayload](#schema-Session.Inbox.UserPayload)

</div>

<div class="api-schema-property">

`delivery`[Session.Inbox.Delivery](#schema-Session.Inbox.Delivery)

</div>

</div>

</div>

</div>

`Session.Inbox.UserPayload`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.FileAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.FileAttachment](#schema-Prompt.FileAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`agents`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.AgentAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.AgentAttachment](#schema-Prompt.AgentAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`skills`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.SkillAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.SkillAttachment](#schema-Prompt.SkillAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`parentID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`fork`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^ses

</div>

</div>

</div>

<div class="api-schema-property">

`boundary`[Session.ForkBoundary](#schema-Session.ForkBoundary)

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

<div class="api-schema-property">

`cost`[Money.USD](#schema-Money.USD)

</div>

<div class="api-schema-property">

`tokens`[TokenUsage.Info](#schema-TokenUsage.Info)

</div>

<div class="api-schema-property">

`outcome`

<div class="api-schema-node">

<div class="api-schema-heading">

`"succeeded" | "failed" | "interrupted"`

</div>

<div class="api-schema-values">

<span>Values</span>`"succeeded" | "failed" | "interrupted"`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`updated`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`idle`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`viewed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`archived`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`subpath`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`[Session.Metadata](#schema-Session.Metadata)

</div>

<div class="api-schema-property">

`permissions`[Permission.Ruleset](#schema-Permission.Ruleset)

</div>

<div class="api-schema-property">

`revert`[Session.Revert](#schema-Session.Revert)

</div>

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

</div>

</div>

</div>

`Session.Message.AgentSelected`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"agent-switched"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"agent-switched"`

</div>

</div>

</div>

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`previous`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Assistant`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`streamed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`completed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"assistant"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"assistant"`

</div>

</div>

</div>

<div class="api-schema-property">

`agent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

<div class="api-schema-property">

`content`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Assistant.Text | Session.Message.Assistant.Reasoning | Session.Message.Assistant.Tool[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Assistant.Text | Session.Message.Assistant.Reasoning | Session.Message.Assistant.Tool`

</div>

<div class="api-schema-variants">

<div>

[Session.Message.Assistant.Text](#schema-Session.Message.Assistant.Text)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Assistant.Reasoning](#schema-Session.Message.Assistant.Reasoning)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Assistant.Tool](#schema-Session.Message.Assistant.Tool)

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`snapshot`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`start`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`end`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`finish`

<div class="api-schema-node">

<div class="api-schema-heading">

`"stop" | "length" | "tool-calls" | "content-filter" | "error" | "unknown"`

</div>

<div class="api-schema-values">

<span>Values</span>`"stop" | "length" | "tool-calls" | "content-filter" | "error" | "unknown"`

</div>

</div>

</div>

<div class="api-schema-property">

`rawFinish`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`providerState`[Session.Message.ProviderState\_4](#schema-Session.Message.ProviderState_4)

</div>

<div class="api-schema-property">

`cost`[Money.USD](#schema-Money.USD)

</div>

<div class="api-schema-property">

`tokens`[TokenUsage.Info](#schema-TokenUsage.Info)

</div>

<div class="api-schema-property">

`error`[Session.StructuredError](#schema-Session.StructuredError)

</div>

<div class="api-schema-property">

`retry`[Session.Message.Assistant.Retry](#schema-Session.Message.Assistant.Retry)

</div>

</div>

</div>

</div>

`Session.Message.Assistant.Reasoning`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"reasoning"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"reasoning"`

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`state`[Session.Message.ProviderState\_1](#schema-Session.Message.ProviderState_1)

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`completed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Assistant.Retry`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`attempt`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

\> 0

</div>

</div>

</div>

<div class="api-schema-property">

`at`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`error`[Session.StructuredError](#schema-Session.StructuredError)

</div>

</div>

</div>

</div>

`Session.Message.Assistant.Text`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"text"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"text"`

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`state`[Session.Message.ProviderState](#schema-Session.Message.ProviderState)

</div>

</div>

</div>

</div>

`Session.Message.Assistant.Tool`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"tool"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"tool"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`executed`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`providerState`[Session.Message.ProviderState\_2](#schema-Session.Message.ProviderState_2)

</div>

<div class="api-schema-property">

`providerResultState`[Session.Message.ProviderState\_3](#schema-Session.Message.ProviderState_3)

</div>

<div class="api-schema-property">

`state`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.ToolState.Streaming | Session.Message.ToolState.Running | Session.Message.ToolState.Completed | Session.Message.ToolState.Error`<span class="api-required">required</span>

</div>

<div class="api-schema-variants">

<div>

[Session.Message.ToolState.Streaming](#schema-Session.Message.ToolState.Streaming)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.ToolState.Running](#schema-Session.Message.ToolState.Running)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.ToolState.Completed](#schema-Session.Message.ToolState.Completed)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.ToolState.Error](#schema-Session.Message.ToolState.Error)

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`ran`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`completed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Compaction`<span>Session.Message.Compaction.Running | Session.Message.Compaction.Completed | Session.Message.Compaction.Failed</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Compaction.Running | Session.Message.Compaction.Completed | Session.Message.Compaction.Failed`

</div>

<div class="api-schema-variants">

<div>

[Session.Message.Compaction.Running](#schema-Session.Message.Compaction.Running)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Compaction.Completed](#schema-Session.Message.Compaction.Completed)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Compaction.Failed](#schema-Session.Message.Compaction.Failed)

</div>

</div>

</div>

</div>

`Session.Message.Compaction.Completed`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"compaction"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"compaction"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"completed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"completed"`

</div>

</div>

</div>

<div class="api-schema-property">

`reason`

<div class="api-schema-node">

<div class="api-schema-heading">

`"auto" | "manual"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"auto" | "manual"`

</div>

</div>

</div>

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

<div class="api-schema-property">

`providerState`[Session.Message.ProviderState\_5](#schema-Session.Message.ProviderState_5)

</div>

<div class="api-schema-property">

`summary`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`recent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`providerContext`[Session.ProviderContext](#schema-Session.ProviderContext)

</div>

<div class="api-schema-property">

`cost`[Money.USD](#schema-Money.USD)

</div>

<div class="api-schema-property">

`tokens`[TokenUsage.Info](#schema-TokenUsage.Info)

</div>

</div>

</div>

</div>

`Session.Message.Compaction.Failed`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"compaction"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"compaction"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"failed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"failed"`

</div>

</div>

</div>

<div class="api-schema-property">

`reason`

<div class="api-schema-node">

<div class="api-schema-heading">

`"auto" | "manual"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"auto" | "manual"`

</div>

</div>

</div>

<div class="api-schema-property">

`error`[Session.StructuredError](#schema-Session.StructuredError)

</div>

<div class="api-schema-property">

`cost`[Money.USD](#schema-Money.USD)

</div>

<div class="api-schema-property">

`tokens`[TokenUsage.Info](#schema-TokenUsage.Info)

</div>

</div>

</div>

</div>

`Session.Message.Compaction.Running`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"compaction"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"compaction"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running"`

</div>

</div>

</div>

<div class="api-schema-property">

`reason`

<div class="api-schema-node">

<div class="api-schema-heading">

`"auto" | "manual"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"auto" | "manual"`

</div>

</div>

</div>

<div class="api-schema-property">

`summary`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`recent`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Idle`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"idle"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"idle"`

</div>

</div>

</div>

<div class="api-schema-property">

`outcome`

<div class="api-schema-node">

<div class="api-schema-heading">

`"succeeded" | "failed" | "interrupted"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"succeeded" | "failed" | "interrupted"`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Info`<span>Session.Message.AgentSelected | Session.Message.ModelSelected | Session.Message.LocationSwitched | Session.Message.User | Session.Message.Synthetic | Session.Message.System | Session.Message.Skill | Session.Message.Shell | Session.Message.Assistant | Session.Message.Compaction | Session.Message.Idle</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.AgentSelected | Session.Message.ModelSelected | Session.Message.LocationSwitched | Session.Message.User | Session.Message.Synthetic | Session.Message.System | Session.Message.Skill | Session.Message.Shell | Session.Message.Assistant | Session.Message.Compaction | Session.Message.Idle`

</div>

<div class="api-schema-variants">

<div>

[Session.Message.AgentSelected](#schema-Session.Message.AgentSelected)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.ModelSelected](#schema-Session.Message.ModelSelected)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.LocationSwitched](#schema-Session.Message.LocationSwitched)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.User](#schema-Session.Message.User)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Synthetic](#schema-Session.Message.Synthetic)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.System](#schema-Session.Message.System)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Skill](#schema-Session.Message.Skill)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Shell](#schema-Session.Message.Shell)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Assistant](#schema-Session.Message.Assistant)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Compaction](#schema-Session.Message.Compaction)

</div>

<div>

<span class="api-schema-operator">or</span>[Session.Message.Idle](#schema-Session.Message.Idle)

</div>

</div>

</div>

</div>

`Session.Message.LocationSwitched`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"location-switched"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"location-switched"`

</div>

</div>

</div>

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`subpath`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`previous`

<div class="api-schema-node">

<div class="api-schema-heading">

`object | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`location`[Location.PublicRef](#schema-Location.PublicRef)

</div>

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`subpath`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.ModelSelected`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"model-switched"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"model-switched"`

</div>

</div>

</div>

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

<div class="api-schema-property">

`previous`[Model.Ref](#schema-Model.Ref)

</div>

</div>

</div>

</div>

`Session.Message.ProviderState`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.Message.ProviderState_1`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.Message.ProviderState_2`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.Message.ProviderState_3`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.Message.ProviderState_4`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.Message.ProviderState_5`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.Message.Shell`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`completed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"shell"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"shell"`

</div>

</div>

</div>

<div class="api-schema-property">

`shellID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^sh\_

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running" | "exited" | "timeout" | "killed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running" | "exited" | "timeout" | "killed"`

</div>

</div>

</div>

<div class="api-schema-property">

`exit`

<div class="api-schema-node">

<div class="api-schema-heading">

`number | "Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`"Infinity" | "-Infinity" | "NaN"`

</div>

<div class="api-schema-values">

<span>Values</span>`"Infinity" | "-Infinity" | "NaN"`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cursor`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`size`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`truncated`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Skill`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"skill"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"skill"`

</div>

</div>

</div>

<div class="api-schema-property">

`skill`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.Synthetic`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"synthetic"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"synthetic"`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.System`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"system"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"system"`

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.ToolState.Completed`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"completed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"completed"`

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`content`

<div class="api-schema-node">

<div class="api-schema-heading">

`Tool.Content[]`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min items 1

</div>

<div class="api-schema-items">

<span>Items</span>[Tool.Content](#schema-Tool.Content)

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.ToolState.Error`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"error"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"error"`

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`error`[Session.StructuredError](#schema-Session.StructuredError)

</div>

<div class="api-schema-property">

`content`

<div class="api-schema-node">

<div class="api-schema-heading">

`Tool.Content[]`

</div>

<div class="api-schema-constraints">

min items 1

</div>

<div class="api-schema-items">

<span>Items</span>[Tool.Content](#schema-Tool.Content)

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.ToolState.Running`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running"`

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.ToolState.Streaming`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"streaming"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"streaming"`

</div>

</div>

</div>

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Message.User`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`created`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.FileAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.FileAttachment](#schema-Prompt.FileAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`agents`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.AgentAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.AgentAttachment](#schema-Prompt.AgentAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`skills`

<div class="api-schema-node">

<div class="api-schema-heading">

`Prompt.SkillAttachment[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Prompt.SkillAttachment](#schema-Prompt.SkillAttachment)

</div>

</div>

</div>

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"user"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"user"`

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Metadata`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

</div>

</div>

`Session.ProviderContext`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`version`

<div class="api-schema-node">

<div class="api-schema-heading">

`1`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`1`

</div>

</div>

</div>

<div class="api-schema-property">

`provenance`[Session.ProviderContext.Provenance](#schema-Session.ProviderContext.Provenance)

</div>

<div class="api-schema-property">

`messages`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.ProviderContext.Provenance`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`provider`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`modelID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`route`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`protocol`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`endpoint`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Session.Revert`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`messageID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^msg\_

</div>

</div>

</div>

<div class="api-schema-property">

`partID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`snapshot`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`files`

<div class="api-schema-node">

<div class="api-schema-heading">

`FileDiff.Info[]`

</div>

<div class="api-schema-items">

<span>Items</span>[FileDiff.Info](#schema-FileDiff.Info)

</div>

</div>

</div>

</div>

</div>

</div>

`Session.StructuredError`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 100, max 599

</div>

</div>

</div>

</div>

</div>

</div>

`SessionActive`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running"`

</div>

</div>

</div>

</div>

</div>

</div>

`SessionBusyErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"SessionBusyError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"SessionBusyError"`

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`SessionGenerateResponse`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`SessionInterruptResponse`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`interrupted`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

Whether an active execution owned by this OpenCode process was interrupted.

</div>

</div>

</div>

</div>

</div>

`SessionLogItemEncoded`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

`SessionMessagesResponse`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Session.Message.Info](#schema-Session.Message.Info)

</div>

</div>

</div>

<div class="api-schema-property">

`cursor`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`previous`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`next`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`SessionNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"SessionNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"SessionNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`sessionID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`SessionStats.Activity`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`date`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`steps`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

`SessionStats.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`range`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`from`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`to`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`sessions`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`subagents`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`prompts`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`steps`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`tokens`[TokenUsage.Info](#schema-TokenUsage.Info)

</div>

<div class="api-schema-property">

`cost`[Money.USD](#schema-Money.USD)

</div>

<div class="api-schema-property">

`tools`[SessionStats.Tools](#schema-SessionStats.Tools)

</div>

<div class="api-schema-property">

`activeDays`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`streak`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`activity`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionStats.Activity[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[SessionStats.Activity](#schema-SessionStats.Activity)

</div>

</div>

</div>

<div class="api-schema-property">

`models`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionStats.ModelUsage[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[SessionStats.ModelUsage](#schema-SessionStats.ModelUsage)

</div>

</div>

</div>

</div>

</div>

</div>

`SessionStats.ModelUsage`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`model`[Model.Ref](#schema-Model.Ref)

</div>

<div class="api-schema-property">

`steps`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`tokens`[TokenUsage.Info](#schema-TokenUsage.Info)

</div>

<div class="api-schema-property">

`cost`[Money.USD](#schema-Money.USD)

</div>

</div>

</div>

</div>

`SessionStats.ToolTotals`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`calls`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`succeeded`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`failed`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`unfinished`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

</div>

</div>

</div>

`SessionStats.ToolUsage`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`calls`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`succeeded`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`failed`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`unfinished`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`durationP50`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

`SessionStats.Tools`<span>object | object | object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object | object | object`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`mode`

<div class="api-schema-node">

<div class="api-schema-heading">

`"none"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"none"`

</div>

</div>

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`mode`

<div class="api-schema-node">

<div class="api-schema-heading">

`"summary"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"summary"`

</div>

</div>

</div>

<div class="api-schema-property">

`totals`[SessionStats.ToolTotals](#schema-SessionStats.ToolTotals)

</div>

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`mode`

<div class="api-schema-node">

<div class="api-schema-heading">

`"detail"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"detail"`

</div>

</div>

</div>

<div class="api-schema-property">

`totals`[SessionStats.ToolTotals](#schema-SessionStats.ToolTotals)

</div>

<div class="api-schema-property">

`usage`

<div class="api-schema-node">

<div class="api-schema-heading">

`SessionStats.ToolUsage[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[SessionStats.ToolUsage](#schema-SessionStats.ToolUsage)

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`SessionTransfer.Data`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`info`[Session.Info](#schema-Session.Info)

</div>

<div class="api-schema-property">

`messages`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Message.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Session.Message.Info](#schema-Session.Message.Info)

</div>

</div>

</div>

</div>

</div>

</div>

`SessionsResponse`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`Session.Info[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[Session.Info](#schema-Session.Info)

</div>

</div>

</div>

<div class="api-schema-property">

`cursor`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`previous`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

<div class="api-schema-property">

`next`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Shell.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

pattern ^sh\_

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"running" | "exited" | "timeout" | "killed"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"running" | "exited" | "timeout" | "killed"`

</div>

</div>

</div>

<div class="api-schema-property">

`command`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cwd`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`shell`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`file`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`pid`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`exit`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

<div class="api-schema-property">

`metadata`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`started`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

Start time in milliseconds since the Unix epoch

</div>

</div>

<div class="api-schema-property">

`completed`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`ShellNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"ShellNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"ShellNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Skill.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`description`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`autoinvoke`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div class="api-schema-property">

`path`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`content`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`SkillNotFoundErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"SkillNotFoundError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"SkillNotFoundError"`

</div>

</div>

</div>

<div class="api-schema-property">

`skill`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`TokenUsage.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`input`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`output`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`reasoning`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`cache`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`read`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`write`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Tool.Content`<span>Tool.TextContent | Tool.FileContent</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Tool.TextContent | Tool.FileContent`

</div>

<div class="api-schema-variants">

<div>

[Tool.TextContent](#schema-Tool.TextContent)

</div>

<div>

<span class="api-schema-operator">or</span>[Tool.FileContent](#schema-Tool.FileContent)

</div>

</div>

</div>

</div>

`Tool.FileContent`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"file"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"file"`

</div>

</div>

</div>

<div class="api-schema-property">

`uri`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`mime`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Tool.TextContent`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`type`

<div class="api-schema-node">

<div class="api-schema-heading">

`"text"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"text"`

</div>

</div>

</div>

<div class="api-schema-property">

`text`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`UnauthorizedErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"UnauthorizedError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"UnauthorizedError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`UnknownErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`_tag`

<div class="api-schema-node">

<div class="api-schema-heading">

`"UnknownError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"UnknownError"`

</div>

</div>

</div>

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`ref`

<div class="api-schema-node">

<div class="api-schema-heading">

`string | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`V2EventEncoded`<span>string</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

`Vcs.Base`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`ref`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`source`

<div class="api-schema-node">

<div class="api-schema-heading">

`"reflog" | "default"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"reflog" | "default"`

</div>

</div>

</div>

</div>

</div>

</div>

`Vcs.Branch`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`current`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`default`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Vcs.BranchList`<span>string\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`string[]`

</div>

<div class="api-schema-items">

<span>Items</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

`Vcs.FileStatus`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`file`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`additions`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`deletions`

<div class="api-schema-node">

<div class="api-schema-heading">

`integer`<span class="api-required">required</span>

</div>

<div class="api-schema-constraints">

min 0

</div>

</div>

</div>

<div class="api-schema-property">

`status`

<div class="api-schema-node">

<div class="api-schema-heading">

`"added" | "deleted" | "modified"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"added" | "deleted" | "modified"`

</div>

</div>

</div>

</div>

</div>

</div>

`Vcs.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`provider`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`branch`[Vcs.Branch](#schema-Vcs.Branch)

</div>

</div>

</div>

</div>

`Vcs.Mode`<span>"working" | "branch" | "committed"</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`"working" | "branch" | "committed"`

</div>

<div class="api-schema-values">

<span>Values</span>`"working" | "branch" | "committed"`

</div>

</div>

</div>

`WebSearch.Provider`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`id`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`WebSearch.ResponseEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`providerID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`results`

<div class="api-schema-node">

<div class="api-schema-heading">

`WebSearch.Result[]`<span class="api-required">required</span>

</div>

<div class="api-schema-items">

<span>Items</span>[WebSearch.Result](#schema-WebSearch.Result)

</div>

</div>

</div>

</div>

</div>

</div>

`WebSearch.Result`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`url`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`title`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`content`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`time`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`published`

<div class="api-schema-node">

<div class="api-schema-heading">

`number`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

`Worktree.CreateInput`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`from`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`branch`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Worktree.Directory`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`strategy`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`

</div>

</div>

</div>

</div>

</div>

</div>

`Worktree.Info`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`Worktree.List`<span>Worktree.Directory\[\]</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`Worktree.Directory[]`

</div>

<div class="api-schema-items">

<span>Items</span>[Worktree.Directory](#schema-Worktree.Directory)

</div>

</div>

</div>

`Worktree.RemoveInput`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`projectID`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`directory`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`force`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`<span class="api-required">required</span>

</div>

</div>

</div>

</div>

</div>

</div>

`WorktreeErrorEncoded`<span>object</span>

<div class="api-schema-body">

<div class="api-schema-node">

<div class="api-schema-heading">

`object`

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`name`

<div class="api-schema-node">

<div class="api-schema-heading">

`"WorktreeError"`<span class="api-required">required</span>

</div>

<div class="api-schema-values">

<span>Values</span>`"WorktreeError"`

</div>

</div>

</div>

<div class="api-schema-property">

`data`

<div class="api-schema-node">

<div class="api-schema-heading">

`object`<span class="api-required">required</span>

</div>

<div class="api-schema-properties">

<div class="api-schema-property">

`message`

<div class="api-schema-node">

<div class="api-schema-heading">

`string`<span class="api-required">required</span>

</div>

</div>

</div>

<div class="api-schema-property">

`forceRequired`

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean | null`

</div>

<div class="api-schema-variants">

<div>

<div class="api-schema-node">

<div class="api-schema-heading">

`boolean`

</div>

</div>

</div>

<div>

<span class="api-schema-operator">or</span>

<div class="api-schema-node">

<div class="api-schema-heading">

`null`

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>

</div>
