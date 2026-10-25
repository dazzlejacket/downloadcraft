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
slice of `len` elements and copies all of them, so this reads 2048 elements
starting at the name: 4 KB on Windows, 8 KB elsewhere.

The pointer UnRAR passes for the notification is `NextName.data()`, the buffer
of its own `std::wstring`. That buffer is only as long as the name — a path of
a few hundred bytes — so the copy reads past its end. When the memory behind
the name happens to be unmapped, the read faults and the process dies with an
access violation (`0xC0000005`) inside the C++ runtime.

The patched code measures the name first and then copies only that much:

```rust
let next = unsafe {
    let base = p1 as *const native::WCHAR;
    let mut len = 0usize;
    if !base.is_null() {
        while len < MAX_VOLUME_NAME_CHARS && *base.add(len) != 0 {
            len += 1;
        }
    }
    widestring::WideCString::from_ptr_truncate(base as *const _, len)
};
```

`native::WCHAR` is the platform `wchar_t`, which is what UnRAR's
`std::wstring` holds: 16 bit on Windows, 32 bit elsewhere. `WideCString` is
`U16CString` on Windows and `U32CString` everywhere else, so the copy lets
`WideCString`'s own signature pick the element type and the two always agree.
The measurement stops at the first nul, so the copy never reads past the name.

`UnRAR`'s own ask-the-caller path (`DllVolChange`) passes a buffer of
`MAXPATHSIZE` (65536) elements, so reading a fixed amount would have been safe
there; the notification path (`DllVolNotify`) passes the short string, and that
is the one that was read out of bounds.

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
