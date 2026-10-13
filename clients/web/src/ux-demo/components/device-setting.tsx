import { Select } from '@kerfjs/ui/select';

/** Which client the demo renders for: the computer running Hot Sheet, or a remote device (HS2-NWWF1Z). */
export type DemoDevice = 'host' | 'remote';

/**
 * The shared settings control for a component's `hostActionsAvailable` state (HS2-TYS5SS): on a
 * remote device, host-only actions render disabled with their explanation.
 */
export function DeviceSetting({ value }: { value: DemoDevice }) {
  return (
    <Select
      name="device"
      label="Device"
      value={value}
      choices={[
        { value: 'host', label: 'Computer running Hot Sheet' },
        { value: 'remote', label: 'Remote device' },
      ]}
    />
  );
}
