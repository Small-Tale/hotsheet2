import {describe,expect,it} from 'vitest';

import {ProviderIcon} from './provider-icon';

describe('ProviderIcon',()=>{it.each(['github','gitlab','jira'] as const)('renders the %s provider identity',kind=>{const markup=String(ProviderIcon({kind}));expect(markup).toContain(`data-provider-icon="${kind}"`);expect(markup).toContain(`aria-label="${kind==='github'?'GitHub':kind==='gitlab'?'GitLab':'Jira'}"`)})});
