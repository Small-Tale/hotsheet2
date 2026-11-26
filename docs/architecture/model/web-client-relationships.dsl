// web-client relationships. Owned by HS2-AYKHVV. Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

workspaceQueriesApi = webWorkspace -> webApi "Requests scoped reads and writes" "TypeScript"
workspaceUsesChanges = webWorkspace -> webChanges "Subscribes to active projects and applies events by cursor" "Worker messages"
