import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';

type Role = components['schemas']['Role'];

/** 網羅しなかった値は既定値に落とさず、値そのものを返す (→ content-labels.ts)。 */
export function roleLabel(role: Role): string {
	if (role === 'admin') return m.admin_users_role_admin();
	if (role === 'user') return m.admin_users_role_user();
	return role;
}
