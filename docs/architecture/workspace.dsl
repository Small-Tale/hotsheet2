!const SOURCE "https://github.com/Small-Tale/hotsheet2/blob/52010ebaf629b1ed95850cf4bb64b1385a03e6e4"

workspace "Hot Sheet 2" "Current architecture at the 2026-11-25 baseline. See the architecture coverage matrix for evidence and limits." {
    !impliedRelationships false

    model {
        !include model.dsl
        !include relationships.dsl
        !include deployment.dsl
    }

    views {
        !include views.dsl

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
