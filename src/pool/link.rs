use std::{marker::PhantomData, mem::MaybeUninit, ptr::addr_of};

#[derive(Debug, Clone)]
pub struct SystemLink {
    pub(crate) from_system_idx: usize,
    pub(crate) to_system_idx: usize,
    pub(crate) to_input_offset: usize,
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
        })
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

        unsafe {
            t_ptr = addr_of!((*base_ptr).0);
            u_ptr = addr_of!((*base_ptr).1);
        }

        let t_off = t_ptr.addr() - base_ptr.addr();
        let u_off = u_ptr.addr() - base_ptr.addr();

        for tl in t_lay.iter_mut() {
            tl.offset += t_off;
        }
        for ul in u_lay.iter_mut() {
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

        unsafe {
            t_ptr = addr_of!((*base_ptr).0);
            u_ptr = addr_of!((*base_ptr).1);
            v_ptr = addr_of!((*base_ptr).2);
        }

        let t_off = t_ptr.addr() - base_ptr.addr();
        let u_off = u_ptr.addr() - base_ptr.addr();
        let v_off = v_ptr.addr() - base_ptr.addr();

        for tl in t_lay.iter_mut() {
            tl.offset += t_off;
        }
        for ul in u_lay.iter_mut() {
            ul.offset += u_off;
        }
        for vl in v_lay.iter_mut() {
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
