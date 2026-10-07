import './ticket-source-icon.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Plug } from 'lucide';

import { customizationContrastColor, isTransparentCommandColor } from './customization-palette';

export interface TicketSourceIdentity {
  provider: string;
  name: string;
  color?: string;
}

/** Project-local source badge, with the provider's line mark and an optional palette background. */
export function TicketSourceIcon({
  source,
  size = 'list',
}: {
  source: TicketSourceIdentity;
  size?: 'list' | 'compact';
}) {
  const filled = !isTransparentCommandColor(source.color);
  const color = filled ? source.color! : undefined;
  return (
    <span
      class="ticket-source-icon"
      data-component="ticket-source-icon"
      data-size={size}
      data-provider={source.provider}
      title={`${source.name} source`}
      aria-label={`${source.name} source`}
      style={color ? `background-color: ${color}; color: ${customizationContrastColor(color)}` : undefined}
    >
      {source.provider === 'git' ? (
        <svg
          class="ticket-source-icon__mark"
          viewBox="0 0 272 355"
          fill="none"
          stroke="currentColor"
          stroke-width="18"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M179.068794,289.715434 C168.109546,267.9821 145.57837,259.876434 133.194843,236.896751 C129.148683,229.388432 126.831101,221.311993 126.034811,212.794291 C125.150416,203.334151 125.499384,193.909889 128.24411,184.881596 L129.696915,180.102846 C122.656971,184.840619 116.377985,190.181471 110.265751,195.971645 C90.5857173,214.614715 74.5591518,240.251873 70.4604953,267.357665 C65.086749,299.808525 75.9734684,331.749554 99.331257,355 C64.8461884,345.828052 34.4358304,326.023578 16.1945608,295.205055 C7.4560238,280.441345 2.47028568,264.327077 0.606734886,247.304231 C-0.992865792,232.692473 0.586006164,218.213122 5.15882706,204.317067 C7.10682342,198.397424 9.36857201,192.928561 12.2408732,187.440143 C14.5092337,183.10574 16.7567907,179.022834 19.4453034,174.913498 C27.6438312,162.382216 35.1265427,153.216507 44.9125531,141.948161 L65.0921381,118.711876 C70.9256048,111.994786 76.331196,105.219916 81.5221623,98.0248322 C89.3658395,87.1528841 95.5027396,76.5237919 100.239305,63.9553835 C106.368678,47.6911807 107.820199,30.1133836 104.729023,13.007114 C103.985127,8.89046875 103.181493,5.26105423 101.8172,1.273662 C101.807206,1.28115495 102.02572,1.11561119 102.177656,1 C121.474702,12.2126907 137.792616,24.7771809 152.820722,41.3173398 C174.025396,64.6555214 188.628698,92.8713982 192.561557,124.386562 C194.341231,138.647613 193.940439,152.94296 192.036402,167.120236 C191.814519,168.772354 191.636658,170.158189 191.726387,171.761862 C191.920089,175.223776 194.954802,177.584942 198.473867,176.986583 C204.511656,175.959957 210.883595,168.681129 214.078456,163.766355 C222.755147,150.41868 224.69971,133.940416 221.741156,118.487094 C221.766911,118.301383 221.817391,118.188293 221.9261,118.178155 C228.837348,124.720838 234.899028,131.726281 240.437795,139.450693 C245.118272,145.978125 249.433751,152.603475 253.24875,159.687548 C267.940394,186.968468 275.530044,219.506955 270.397511,250.336999 C268.844264,259.667031 266.34128,268.680244 262.646257,277.382417 C252.887914,300.364356 236.428448,319.932377 215.707593,333.847521 C206.781622,339.841779 197.464705,344.7752 187.377762,348.664001 C181.034019,351.109693 174.048605,353.371866 167.676177,354.952557 C167.431071,355.013356 166.949457,354.963033 167.005671,354.903055 L167.31279,354.575369 C173.434925,348.043251 178.436735,340.670987 181.5053,332.216833 C186.786378,317.667016 186.020326,303.501049 179.068794,289.715434 L179.068794,289.715434 Z" />
        </svg>
      ) : source.provider === 'github' ? (
        <svg
          class="ticket-source-icon__mark"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M9 19c-4.5 1.4-4.5-2-6-2.5M9 21v-3.4c-3.3-.5-5-2.4-5-5.4 0-1.5.5-2.7 1.4-3.6-.2-.8-.2-1.9.2-3 1.2 0 2.4.5 3.3 1.3a12 12 0 0 1 6.2 0c.9-.8 2.1-1.3 3.3-1.3.4 1.1.4 2.2.2 3 .9.9 1.4 2.1 1.4 3.6 0 3-1.7 4.9-5 5.4V21" />
        </svg>
      ) : source.provider === 'gitlab' ? (
        <svg
          class="ticket-source-icon__mark"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="m12 22 10-8.1-2.3-11-3.5 6H7.8l-3.5-6L2 13.9 12 22Z" />
          <path d="m7.8 8.9 4.2 13.1 4.2-13.1" />
        </svg>
      ) : source.provider === 'jira' ? (
        <svg
          class="ticket-source-icon__mark"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          aria-hidden="true"
        >
          <path d="M12 2 22 12 12 22 2 12 12 2Z" />
          <path d="m12 7 5 5-5 5-5-5 5-5Z" />
        </svg>
      ) : (
        <LucideIcon icon={Plug} name={source.provider} size={size === 'list' ? 26.4 : 15.2} />
      )}
    </span>
  );
}
