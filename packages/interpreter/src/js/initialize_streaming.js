window.hydrate_queue=[];window.dx_hydrate=(id,data,debug_types,debug_locations)=>{const decoded=atob(data),bytes=Uint8Array.from(decoded,(c)=>c.charCodeAt(0));if(window.hydration_callback)window.hydration_callback(id,bytes,debug_types,debug_locations);else window.hydrate_queue.push([id,bytes,debug_types,debug_locations])};
