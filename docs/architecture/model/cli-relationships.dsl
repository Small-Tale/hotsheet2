// cli relationships. Owned by HS2-TW2DKM. Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

toolCallsMcp = aiTool -> mcp "Invokes checkout tools" "stdio JSON-RPC"
cliUsesGithub = cli -> github "Runs supported provider and CI operations" "HTTPS"
cliUsesGitlab = cli -> gitlab "Runs supported provider and CI operations" "HTTPS"
cliUsesJira = cli -> jira "Runs supported provider operations" "HTTPS"
cliSyncsGit = cli -> gitRemote "Initializes, links, and syncs repositories on request" "Git"
cliUsesOs = cli -> osServices "Uses native credentials and starts local tools" "Native OS APIs"
cliRunsTool = cli -> aiTool "Starts compatible tools for headless launch and drive operations" "Process launch"
migrationReadsLegacy = migrator -> legacy "Exports from a copy of the old database" "Node.js / PGLite"
