use std::{marker::PhantomData, mem::MaybeUninit, ptr::addr_of};

#[derive(Debug, Clone)]
pub struct SystemLink {
    pub(crate) from_system_idx: usize,
    pub(crate) to_system_idx: usize,
    #[expect(
        dead_code,
        reason = "Phase 4 deletes both fields. They are flat-buffer bookkeeping that \
                  slice-per-link made unnecessary — the solver reads neither, which \
                  is why the compiler reports them as never read. Removing them now \
                  means also removing `SystemLayout` and `SystemOut::layouts`, which \
                  is Phase 4's whole content. See specs/roadmap.md Phase 4."
    )]
    pub(crate) to_input_offset: usize,
    #[expect(
        dead_code,
        reason = "Phase 4 deletes this, see `to_input_offset` above."
    )]
    pub(crate) num_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct SystemLayout {
    offset: usize,
    len: usize,
}

pub trait SystemIn {
    type In;
    fn id(&self) -> usize;
}
pub trait SystemOut {
    type Out;
    fn ids(&self) -> &[usize];
    fn layouts(&self) -> Vec<SystemLayout>;
    fn add_links_to(&self, to_id: usize, base_offset: usize, links: &mut Vec<SystemLink>);
}

#[derive(Clone, Copy)]
pub struct SystemRef<In, Out> {
    pub id: [usize; 1],
    pub(crate) _io: PhantomData<(In, Out)>,
}

impl<In, Out> SystemIn for SystemRef<In, Out> {
    type In = In;
    fn id(&self) -> usize {
        self.id[0]
    }
}
impl<In, Out> SystemOut for SystemRef<In, Out> {
    type Out = Out;
    fn ids(&self) -> &[usize] {
        &self.id
    }

    fn layouts(&self) -> Vec<SystemLayout> {
        vec![SystemLayout {
            offset: 0,
            len: size_of::<Out>(),
        }]
    }

    fn add_links_to(&self, to_id: usize, base_offset: usize, links: &mut Vec<SystemLink>) {
        links.push(SystemLink {
            from_system_idx: self.id[0],
            to_system_idx: to_id,
            to_input_offset: base_offset,
            num_bytes: size_of::<Out>(),
        });
    }
}

pub struct SystemMux<Out> {
    ids: Vec<usize>,
    layouts: Vec<SystemLayout>,
    _io: PhantomData<Out>,
}

impl<Out> SystemOut for SystemMux<Out> {
    type Out = Out;

    fn ids(&self) -> &[usize] {
        &self.ids
    }

    fn layouts(&self) -> Vec<SystemLayout> {
        self.layouts.clone()
    }

    fn add_links_to(&self, to_id: usize, base_offset: usize, links: &mut Vec<SystemLink>) {
        for (i, SystemLayout { offset, len }) in self.layouts.iter().enumerate() {
            links.push(SystemLink {
                from_system_idx: self.ids[i],
                to_system_idx: to_id,
                to_input_offset: base_offset + offset,
                num_bytes: *len,
            });
        }
    }
}

impl<T, U> From<(&T, &U)> for SystemMux<(T::Out, U::Out)>
where
    T: SystemOut,
    U: SystemOut,
{
    fn from((t, u): (&T, &U)) -> Self {
        let ids = t.ids().iter().chain(u.ids().iter()).copied().collect();

        let mut t_lay = t.layouts();
        let mut u_lay = u.layouts();

        let tmp: MaybeUninit<(T::Out, U::Out)> = MaybeUninit::uninit();
        let base_ptr = tmp.as_ptr();

        let t_ptr;
        let u_ptr;

        #[expect(
            clippy::undocumented_unsafe_blocks,
            clippy::multiple_unsafe_ops_per_block,
            reason = "Phase 4 deletes this. The whole `MaybeUninit` + `addr_of!` \
                      offset computation is vestigial: slice-per-link means the \
                      solver never reads `layouts`, and the compiler already reports \
                      `to_input_offset` and `num_bytes` as never read. Documenting \
                      unsafe operations in code scheduled for deletion is waste. See \
                      specs/roadmap.md Phase 4."
        )]
        unsafe {
            t_ptr = addr_of!((*base_ptr).0);
            u_ptr = addr_of!((*base_ptr).1);
        }

        let t_off = t_ptr.addr() - base_ptr.addr();
        let u_off = u_ptr.addr() - base_ptr.addr();

        for tl in &mut t_lay {
            tl.offset += t_off;
        }
        for ul in &mut u_lay {
            ul.offset += u_off;
        }

        t_lay.extend_from_slice(&u_lay);

        Self {
            ids,
            layouts: t_lay,
            _io: PhantomData,
        }
    }
}

impl<T, U, V> From<(&T, &U, &V)> for SystemMux<(T::Out, U::Out, V::Out)>
where
    T: SystemOut,
    U: SystemOut,
    V: SystemOut,
{
    fn from((t, u, v): (&T, &U, &V)) -> Self {
        let ids = t.ids().iter().chain(u.ids().iter()).copied().collect();

        let mut t_lay = t.layouts();
        let mut u_lay = u.layouts();
        let mut v_lay = v.layouts();

        let tmp: MaybeUninit<(T::Out, U::Out, V::Out)> = MaybeUninit::uninit();
        let base_ptr = tmp.as_ptr();

        let t_ptr;
        let u_ptr;
        let v_ptr;

        #[expect(
            clippy::undocumented_unsafe_blocks,
            clippy::multiple_unsafe_ops_per_block,
            reason = "Phase 4 deletes this, for the reason given on the two-input \
                      impl above. Note that this impl also carries the missing \
                      `v.ids()` bug that deletion removes. See specs/roadmap.md \
                      Phase 4."
        )]
        unsafe {
            t_ptr = addr_of!((*base_ptr).0);
            u_ptr = addr_of!((*base_ptr).1);
            v_ptr = addr_of!((*base_ptr).2);
        }

        let t_off = t_ptr.addr() - base_ptr.addr();
        let u_off = u_ptr.addr() - base_ptr.addr();
        let v_off = v_ptr.addr() - base_ptr.addr();

        for tl in &mut t_lay {
            tl.offset += t_off;
        }
        for ul in &mut u_lay {
            ul.offset += u_off;
        }
        for vl in &mut v_lay {
            vl.offset += v_off;
        }

        t_lay.extend_from_slice(&u_lay);
        t_lay.extend_from_slice(&v_lay);

        Self {
            ids,
            layouts: t_lay,
            _io: PhantomData,
        }
    }
}

impl<I, O> From<SystemRef<I, O>> for SystemMux<O> {
    fn from(value: SystemRef<I, O>) -> Self {
        Self {
            ids: value.ids().to_vec(),
            layouts: value.layouts(),
            _io: PhantomData,
        }
    }
}
