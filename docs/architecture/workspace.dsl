!const SOURCE "https://github.com/Small-Tale/hotsheet2/blob/8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774"

workspace "Hot Sheet 2" "Current architecture at the 2026-11-26 deep baseline (HS2-3FA79J). See the architecture coverage matrix for evidence and limits." {
    !impliedRelationships false

    model {
        !include model/context.dsl
        !include model/system.dsl

        // Relationships are included after every element, so files may refer to
        // any identifier. Area files own intra-area edges; cross-area edges are
        // append-only in relationships-cross.dsl.
        !include model/context-relationships.dsl
        !include model/core-relationships.dsl
        !include model/orchestration-relationships.dsl
        !include model/cli-relationships.dsl
        !include model/server-relationships.dsl
        !include model/web-host-relationships.dsl
        !include model/web-client-relationships.dsl
        !include model/relationships-cross.dsl

        !include model/deployment.dsl
    }

    views {
        !include views/shared.dsl
        !include views/core.dsl
        !include views/orchestration.dsl
        !include views/cli.dsl
        !include views/server.dsl
        !include views/web-host.dsl
        !include views/web-client.dsl
        !include views/flows.dsl

        styles {
            element "Person" {
                shape Person
                background #15466b
                color #ffffff
            }
            element "Software System" {
                background #15466b
                color #ffffff
            }
            element "Container" {
                background #235e78
                color #ffffff
            }
            element "Component" {
                background #dceef5
                color #132f3e
            }
            element "External" {
                background #526273
                color #ffffff
            }
            element "Data Store" {
                shape Cylinder
                background #415971
                color #ffffff
            }
            element "Library" {
                shape Component
                background #3d7a8f
                color #ffffff
            }
            element "Derived" {
                background #deedf3
                color #132f3e
            }
            relationship "Relationship" {
                color #44556b
                fontSize 22
            }
        }
    }

    configuration {
        scope softwaresystem
    }
}
