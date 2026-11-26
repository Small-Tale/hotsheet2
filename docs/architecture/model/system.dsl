// The Hot Sheet 2 software system. Shared; HS2-M81CCP owns changes. Each area
// file defines its own containers and components inside this block.

hs2 = softwareSystem "Hot Sheet 2" "Local-first, provider-neutral ticketing, terminal sessions, and coordination across registered checkouts and machines." {
    !docs ../narrative
    !adrs ../decisions

    !include web-client.dsl
    !include web-host.dsl
    !include server.dsl
    !include cli.dsl
    !include core.dsl
    !include orchestration.dsl

    machineData = container "Machine state" "Instance and connection registries, device sessions, local orchestration records, resource claims, journals, and private credential envelopes." "Machine-local files" "Data Store" {
        url "${SOURCE}/crates/hotsheet-server/src/lib.rs"
    }
}
