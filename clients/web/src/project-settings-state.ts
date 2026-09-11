export function projectSettingsValue<T,Fallback extends T>(values:Readonly<Record<string,T>>,projectId:string,fallback:Fallback):T {
  return values[projectId]??fallback;
}

export function updateProjectSettingsValue<T>(values:Readonly<Record<string,T>>,projectId:string,value:T):Record<string,T> {
  return {...values,[projectId]:value};
}

export function discardProjectSettings<T>(values:Readonly<Record<string,T>>,projectIds:ReadonlySet<string>):Record<string,T> {
  return Object.fromEntries(Object.entries(values).filter(([projectId])=>!projectIds.has(projectId)));
}
