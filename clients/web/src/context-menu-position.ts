export interface ContextMenuSize {width:number;height:number}
export interface ContextMenuPosition {x:number;y:number}

export function viewportSafePointerPosition(x:number,y:number,viewportWidth:number,viewportHeight:number):ContextMenuPosition {
  return{x:Math.max(0,Math.min(x,viewportWidth)),y:Math.max(0,Math.min(y,viewportHeight))};
}

export function viewportSafeContextMenuPosition(x:number,y:number,viewportWidth:number,viewportHeight:number,size:ContextMenuSize,margin=8):ContextMenuPosition {
  const availableWidth=Math.max(0,viewportWidth-margin*2),availableHeight=Math.max(0,viewportHeight-margin*2);
  return {
    x:Math.max(margin,Math.min(x,margin+Math.max(0,availableWidth-size.width))),
    y:Math.max(margin,Math.min(y,margin+Math.max(0,availableHeight-size.height))),
  };
}
