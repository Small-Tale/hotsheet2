!const SOURCE "https://github.com/Small-Tale/hotsheet2/blob/8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774"

workspace "Hot Sheet 2" "Current architecture at the 2026-11-26 deep baseline (HS2-3FA79J). See the architecture coverage matrix for evidence and limits." {
    !impliedRelationships false

    model {
        !include model/context.dsl
        !include model/system.dsl
        !include model/overview.dsl

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
        // Guided-tour order (HS2-DHCY18): overview, system, then each area from
        // the screen inward, then the end-to-end flows. The viewer lists views in
        // this order because of structurizr.sort below.
        !include views/tour.dsl
        !include views/shared.dsl
        !include views/web-client.dsl
        !include views/web-host.dsl
        !include views/server.dsl
        !include views/cli.dsl
        !include views/core.dsl
        !include views/orchestration.dsl
        !include views/flows.dsl
        !include views/screenshots.dsl

        properties {
            "structurizr.sort" "created"
            "structurizr.tooltips" "true"
        }

        // Every colour, shape and line setting lives in an explicit light or dark
        // block. The viewer merges same-tag styles in place, so a tag styled both
        // outside and inside a colour-scheme block leaks dark colours into light
        // mode after a dark render (HS2-DHCY18).
        styles {
            element "Element" {
                shape RoundedBox
                strokeWidth 2
                fontSize 22
            }
            element "Overview" {
                width 420
                height 230
                fontSize 26
            }
            light {
                element "Person" {
                    shape Person
                    background #1e4e79
                    stroke #163a5b
                    color #ffffff
                }
                element "Software System" {
                    background #1e4e79
                    stroke #163a5b
                    color #ffffff
                }
                element "Container" {
                    background #2a7ab0
                    stroke #1e5f8a
                    color #ffffff
                }
                element "Component" {
                    background #eaf3fa
                    stroke #2a7ab0
                    color #12324a
                }
                element "External" {
                    background #f3f5f7
                    stroke #8696a7
                    color #2b3a4a
                    border dashed
                }
                element "Data Store" {
                    shape Cylinder
                    background #dbe9f4
                    stroke #2a7ab0
                    color #12324a
                }
                element "Library" {
                    shape Component
                    background #3f8f7a
                    stroke #2f6d5d
                    color #ffffff
                }
                element "Derived" {
                    background #f4f8fb
                    stroke #6f8fa8
                    color #12324a
                    border dashed
                }
                element "Overview People" {
                    shape Person
                    background #1e4e79
                    stroke #163a5b
                    color #ffffff
                }
                element "Overview Client" {
                    background #2a7ab0
                    stroke #1e5f8a
                    color #ffffff
                }
                element "Overview Host" {
                    background #1e4e79
                    stroke #163a5b
                    color #ffffff
                }
                element "Overview Core" {
                    background #3f8f7a
                    stroke #2f6d5d
                    color #ffffff
                }
                element "Overview Store" {
                    shape Cylinder
                    background #dbe9f4
                    stroke #2a7ab0
                    color #12324a
                }
                element "Overview External" {
                    background #f3f5f7
                    stroke #8696a7
                    color #2b3a4a
                    border dashed
                }
                relationship "Relationship" {
                    color #8496aa
                    style solid
                    thickness 2
                    fontSize 20
                    width 260
                }
            }
            dark {
                element "Person" {
                    shape Person
                    background #2a6496
                    stroke #3d7fb5
                    color #ffffff
                }
                element "Software System" {
                    background #2a6496
                    stroke #3d7fb5
                    color #ffffff
                }
                element "Container" {
                    background #2f80b8
                    stroke #4a9bd1
                    color #ffffff
                }
                element "Component" {
                    background #1c2a36
                    stroke #4a9bd1
                    color #e3edf5
                }
                element "External" {
                    background #232a31
                    stroke #7c8a99
                    color #d6dee6
                    border dashed
                }
                element "Data Store" {
                    shape Cylinder
                    background #1b3347
                    stroke #4a9bd1
                    color #e3edf5
                }
                element "Library" {
                    shape Component
                    background #3a8a75
                    stroke #56a891
                    color #ffffff
                }
                element "Derived" {
                    background #1a242d
                    stroke #6f8fa8
                    color #e3edf5
                    border dashed
                }
                element "Overview People" {
                    shape Person
                    background #2a6496
                    stroke #3d7fb5
                    color #ffffff
                }
                element "Overview Client" {
                    background #2f80b8
                    stroke #4a9bd1
                    color #ffffff
                }
                element "Overview Host" {
                    background #2a6496
                    stroke #3d7fb5
                    color #ffffff
                }
                element "Overview Core" {
                    background #3a8a75
                    stroke #56a891
                    color #ffffff
                }
                element "Overview Store" {
                    shape Cylinder
                    background #1b3347
                    stroke #4a9bd1
                    color #e3edf5
                }
                element "Overview External" {
                    background #232a31
                    stroke #7c8a99
                    color #d6dee6
                    border dashed
                }
                relationship "Relationship" {
                    color #8a9bb0
                    style solid
                    thickness 2
                    fontSize 20
                    width 260
                }
            }
        }
    }

    configuration {
        scope softwaresystem
    }
}
