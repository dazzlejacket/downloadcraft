# Patches on top of `unrar` 0.5.8

This directory is the `unrar` crate (0.5.8, MIT OR Apache-2.0) as published on
crates.io, with one change. The root `Cargo.toml` points crates.io at this copy
with a `[patch.crates-io]` entry.

## The change

`src/open_archive.rs`, in the callback UnRAR calls when it moves to the next
volume of a multi-volume set.

Upstream copies a fixed 2048 elements out of the pointer UnRAR passes:

```rust
let next =
    unsafe { widestring::WideCString::from_ptr_truncate(p1 as *const _, 2048) };
```

`WideCString::from_ptr_truncate` does not stop at the first nul. It takes a
slice of `len` elements and copies all of them, so this reads 4 KB starting at
the name.

The pointer UnRAR passes for the notification is `NextName.data()`, the buffer
of its own `std::wstring`. That buffer is only as long as the name — a path of
a few hundred bytes — so the copy reads past its end. When the memory behind
the name happens to be unmapped, the read faults and the process dies with an
access violation (`0xC0000005`) inside the C++ runtime.

The patched code copies the name up to its terminator instead:

```rust
let next = unsafe {
    let mut len = 0usize;
    while len < 2048 && *((p1 as *const u16).add(len)) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(p1 as *const u16, len);
    widestring::WideCString::from_vec_truncate(slice.to_vec())
};
```

`UnRAR`'s own ask-the-caller path (`DllVolChange`) passes a buffer of 2048
elements, so reading that much would have been safe there; the notification
path (`DllVolNotify`) passes the short string, and that is the one that was
read out of bounds.

## Why it is not in the app

The copy happens inside the reader's callback, which the app cannot reach: the
reader registers the callback itself right before it hands the archive to
UnRAR. The app uses the reader's public API for everything else — opening the
first volume of a set, walking the headers, skipping an entry and extracting
one — and that API is unchanged.

## Removing the copy

If the reader publishes a release with this fix, drop the `[patch.crates-io]`
entry from the root `Cargo.toml` and this directory, and the app will use the
published crate again.

## Checking the copy on its own

```
cargo test --manifest-path vendor/unrar/Cargo.toml
```

The reader's own test suite needs its `data/` directory, which is included.
