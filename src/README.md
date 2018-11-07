# Snarc

A 'snitching' atomically reference counted pointer.

A partly drop-in replacement for `Arc` which tracks all references. Useful
when debugging circular references and other undropped pointers.

