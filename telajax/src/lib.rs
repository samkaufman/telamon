// Copyright 2018 Ulysse Beaugnon and Ecole Normale Superieure
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Unsafe wrapper around  library from Kalray.

#![allow(dead_code)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

pub mod opencl;

mod telajax;

use crate::telajax::{
    _cl_event, cl_event, device_t, event_t, kernel_t, mem_t, wrapper_t,
};

use lazy_static::lazy_static;
use libc;
use log::debug;
use parking_lot;
use std::{
    ffi::CStr,
    result::Result,
    sync::{Arc, RwLock},
};

lazy_static! {
    static ref DEVICE: Device = Device::init().unwrap();
}

#[derive(Debug)]
pub enum RWError {
    SizeMismatch(usize, usize),
    CLError(opencl::Error),
}

/// Buffer in MPPA RAM.
/// This is actually a wrapper around Mem that also holds type information and present a safe
/// interface (access protected with RwLock)
pub struct Buffer<T: Copy> {
    pub mem: RwLock<Mem>,
    /// number of T elements (not of bytes like in mem)
    pub len: usize,
    pub executor: &'static Device,
    phantom: std::marker::PhantomData<T>,
}

impl<T: Copy> Buffer<T> {
    pub fn new(executor: &'static Device, len: usize) -> Self {
        let mem_block = executor.alloc(len * std::mem::size_of::<T>()).unwrap();
        Buffer {
            mem: RwLock::new(mem_block),
            len,
            executor,
            phantom: std::marker::PhantomData,
        }
    }

    pub fn raw_ptr(&self) -> *const libc::c_void {
        self.mem.read().unwrap().raw_ptr()
    }

    pub fn read(&self) -> Result<Vec<T>, RWError> {
        let mut data: Vec<T> = Vec::with_capacity(self.len);
        let mem = self.mem.read().unwrap();
        let res = self
            .executor
            .read_buffer(&mem, data.as_mut_slice(), self.len);
        if let Err(rw_error) = res {
            Err(rw_error)
        } else {
            unsafe {
                Vec::set_len(&mut data, self.len);
            }
            Ok(data)
        }
    }

    pub fn write(&self, data: &[T]) -> Result<(), RWError> {
        let mut mem = self.mem.write().unwrap();
        self.executor.write_buffer(data, &mut mem)
    }
}

/// A Telajax execution context.
pub struct Device {
    /// We use an Arc to make sure that no callback is pointing to the device when we try to drop
    /// it
    inner: Arc<device_t>,
}

unsafe impl Sync for Device {}
unsafe impl Send for Device {}

impl Device {
    /// Returns a reference to the `Device`. As Telajax implementation is
    /// supposed to be thread-safe, it should be therefore safe to call
    /// this api from different threads. These calls are done through a
    /// static immutable reference
    /// It appeared that the Kalray OpenCL is actually not thread-safe at all,
    /// see above
    pub fn get() -> &'static Device {
        &DEVICE
    }

    /// Initializes the device.
    fn init() -> Result<Self, opencl::Error> {
        let mut error = 0;
        let device = unsafe {
            Device {
                inner: Arc::new(telajax::device_init(
                    0,
                    std::ptr::null_mut(),
                    &mut error,
                )),
            }
        };
        if error != 0 {
            Err(error.into())
        } else {
            Ok(device)
        }
    }

    /// allocate an array of len T elements on device
    pub fn allocate_array<T: Copy>(&'static self, len: usize) -> Buffer<T> {
        Buffer::new(self, len)
    }

    /// Build a wrapper for a kernel.
    pub fn build_wrapper(
        &self,
        name: &CStr,
        code: &CStr,
    ) -> Result<Wrapper, opencl::Error> {
        let mut error = 0;
        let flags: &'static CStr = Default::default();
        let wrapper = unsafe {
            telajax::wrapper_build(
                name.as_ptr(),
                code.as_ptr(),
                flags.as_ptr(),
                &*self.inner as *const _ as *mut _,
                &mut error,
            )
        };
        if error != 0 {
            Err(error.into())
        } else {
            Ok(Wrapper(wrapper))
        }
    }

    /// Compiles a kernel.
    pub fn build_kernel(
        &self,
        code: &CStr,
        cflags: &CStr,
        lflags: &CStr,
        wrapper: &Wrapper,
    ) -> Result<Kernel, opencl::Error> {
        let mut error = 0;
        {
            let kernel = unsafe {
                telajax::kernel_build(
                    code.as_ptr(),
                    cflags.as_ptr(),
                    lflags.as_ptr(),
                    &wrapper.0 as *const _ as *mut _,
                    &*self.inner as *const _ as *mut _,
                    &mut error,
                )
            };
            if error != 0 {
                Err(error.into())
            } else {
                Ok(Kernel(kernel))
            }
        }
    }

    /// Asynchronously executes a `Kernel`.
    pub fn enqueue_kernel(&self, kernel: &mut Kernel) -> Result<Event, opencl::Error> {
        let mut event = Event::new();
        unsafe {
            let err = telajax::kernel_enqueue(
                &mut kernel.0 as *mut _,
                &self.inner as *const _ as *mut _,
                &mut event.0 as *mut cl_event,
            );
            if err != 0 {
                Err(err.into())
            } else {
                Ok(event)
            }
        }
    }

    /// Print func id then execute it
    pub fn execute_kernel_id(
        &self,
        kernel: &mut Kernel,
        kernel_id: u16,
    ) -> Result<(), opencl::Error> {
        println!("Executing kernel {}", kernel_id);
        self.execute_kernel(kernel)
    }

    /// Executes a `Kernel` and then wait for completion.
    pub fn execute_kernel(&self, kernel: &mut Kernel) -> Result<(), opencl::Error> {
        let mut event = Event::new();
        unsafe {
            let err = telajax::kernel_enqueue(
                &mut kernel.0 as *mut _,
                &*self.inner as *const _ as *mut _,
                &mut event.0 as *mut cl_event,
            );
            if err != 0 {
                return Err(err.into());
            }
            let err = telajax::event_wait(1, &mut event.0 as *mut cl_event);
            if err != 0 {
                Err(err.into())
            } else {
                Ok(())
            }
        }
    }

    /// Waits until all kernels have completed their execution.
    pub fn wait_all(&self) -> Result<(), opencl::Error> {
        unsafe {
            let err = telajax::device_waitall(&*self.inner as *const _ as *mut _);
            if err == 0 {
                Ok(())
            } else {
                Err(err.into())
            }
        }
    }

    /// Allocates a memory buffer. Should only be used by Buffer::new
    fn alloc(&self, size: usize) -> Result<Mem, opencl::Error> {
        let mut error = 0;
        let mem = unsafe {
            telajax::device_mem_alloc(
                size,
                1 << 0,
                &*self.inner as *const _ as *mut _,
                &mut error,
            )
        };
        if error == 0 {
            Ok(Mem {
                ptr: mem,
                len: size,
            })
        } else {
            Err(error.into())
        }
    }

    /// Asynchronously copies a buffer to the device.
    fn async_write_buffer<T: Copy>(
        &self,
        data: &[T],
        mem: &mut Mem,
        wait_events: &[Event],
    ) -> Result<Event, RWError> {
        let size = data.len() * std::mem::size_of::<T>();
        if size > mem.len {
            return Err(RWError::SizeMismatch(size, mem.len));
        }
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let wait_n = wait_events.len() as libc::c_uint;
        let wait_ptr = wait_events.as_ptr() as *const event_t;
        let mut event = Event::new();
        unsafe {
            let res = telajax::device_mem_write(
                &*self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                wait_n,
                wait_ptr,
                &mut event.0 as *mut cl_event,
            );
            if res != 0 {
                Err(RWError::CLError(res.into()))
            } else {
                Ok(event)
            }
        }
    }

    /// Copies a buffer to the device.
    fn write_buffer<T: Copy>(&self, data: &[T], mem: &mut Mem) -> Result<(), RWError> {
        let size = data.len() * std::mem::size_of::<T>();
        if size > mem.len {
            return Err(RWError::SizeMismatch(size, mem.len));
        }
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let wait_ptr = std::ptr::null() as *const event_t;
        let event_ptr = std::ptr::null_mut();
        unsafe {
            let err = telajax::device_mem_write(
                &*self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                0,
                wait_ptr,
                event_ptr,
            );
            if err != 0 {
                Err(RWError::CLError(err.into()))
            } else {
                Ok(())
            }
        }
    }

    /// Asynchronously copies a buffer from the device.
    fn async_read_buffer<T: Copy>(
        &self,
        mem: &Mem,
        data: &mut [T],
        len: usize,
        wait_events: &[Event],
    ) -> Result<Event, RWError> {
        let size = len * std::mem::size_of::<T>();
        if size > mem.len {
            return Err(RWError::SizeMismatch(size, mem.len));
        }
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let wait_n = wait_events.len() as std::os::raw::c_uint;
        let mut event = Event::new();
        {
            // Block for lock drop
            let wait_ptr = if wait_n == 0 {
                std::ptr::null() as *const event_t
            } else {
                wait_events.as_ptr() as *const event_t
            };
            unsafe {
                let res = telajax::device_mem_read(
                    &*self.inner as *const _ as *mut _,
                    mem.ptr,
                    data_ptr,
                    size,
                    wait_n,
                    wait_ptr,
                    &mut event.0 as *mut _,
                );
                if res != 0 {
                    Err(RWError::CLError(res.into()))
                } else {
                    Ok(event)
                }
            }
        }
    }

    /// Copies a buffer from the device.
    fn read_buffer<T: Copy>(
        &self,
        mem: &Mem,
        data: &mut [T],
        len: usize,
    ) -> Result<(), RWError> {
        let size = len * std::mem::size_of::<T>();
        if size > mem.len {
            return Err(RWError::SizeMismatch(size, mem.len));
        }
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let null_mut = std::ptr::null_mut();
        let wait_n = 0;
        let wait_ptr = std::ptr::null() as *const event_t;
        unsafe {
            let res = telajax::device_mem_read(
                &*self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                wait_n,
                wait_ptr,
                null_mut,
            );
            if res != 0 {
                Err(RWError::CLError(res.into()))
            } else {
                Ok(())
            }
        }
    }

    /// Set a callback to call when an event is triggered.
    pub fn set_event_callback<F>(&self, event: &Event, closure: F) -> Result<(), ()>
    where
        F: FnOnce() + Send,
    {
        let callback_data = CallbackData {
            closure,
            arc_device: Arc::clone(&self.inner),
        };
        let data_ptr = Box::into_raw(Box::new(callback_data));
        let callback = callback_wrapper::<F>;
        unsafe {
            let data_ptr = data_ptr as *mut std::os::raw::c_void;
            if telajax::event_set_callback(Some(callback), data_ptr, event.0) == 0 {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            unsafe {
                telajax::device_finalize(inner);
            }
        } else {
            // if None is returned here, this means that some callback still holds a closure to the
            // Device. This is not supposed to happen, so we panic
            panic!("Trying to drop Device while there is still someone holding a reference to it");
        }
        debug!("MPPA device finalized");
    }
}

/// A wrapper openCL that will call the kernel through OpenCL interface
pub struct Wrapper(wrapper_t);
unsafe impl Send for Wrapper {}
unsafe impl Sync for Wrapper {}

impl Drop for Wrapper {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(telajax::wrapper_release(&mut self.0 as *mut _), 0);
        }
    }
}

pub struct Kernel(kernel_t);

unsafe impl Send for Kernel {}

impl Kernel {
    /// Sets the arguments of the `Kernel`.
    pub fn set_args(
        &mut self,
        sizes: &[usize],
        args: &[*const libc::c_void],
    ) -> Result<(), ()> {
        if sizes.len() != args.len() {
            return Err(());
        };
        let num_arg = sizes.len() as i32;
        let sizes_ptr = sizes.as_ptr();
        unsafe {
            // Needs *mut ptr mostly because they were not specified as const in original
            // c api
            if telajax::kernel_set_args(
                num_arg,
                sizes_ptr as *const _ as *mut _,
                args.as_ptr() as *const _ as *mut _,
                &mut self.0 as *mut _,
            ) == 0
            {
                Ok(())
            } else {
                Err(())
            }
        }
    }

    /// Sets the number of clusters that must execute the `Kernel`.
    pub fn set_num_clusters(&mut self, num: usize) -> Result<(), ()> {
        if num > 16 {
            return Err(());
        }
        unsafe {
            if telajax::kernel_set_dim(
                1,
                [1].as_ptr(),
                [num].as_ptr(),
                &mut self.0 as *mut _,
            ) == 0
            {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(telajax::kernel_release(&mut self.0 as *mut _), 0);
        }
    }
}

/// A buffer allocated on the device.
pub struct Mem {
    /// pointer in host memory that will be passed to telajax calls
    ptr: mem_t,
    /// number of bytes allocated on device
    len: usize,
}

unsafe impl Sync for Mem {}
unsafe impl Send for Mem {}

impl Mem {
    /// Telajax_set_args needs to have the size of the ptr for its call
    /// in general, this is just a pointer on host
    pub fn get_mem_size() -> usize {
        std::mem::size_of::<mem_t>()
    }

    pub fn raw_ptr(&self) -> *const libc::c_void {
        &self.ptr as *const _ as *const libc::c_void
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mem {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(telajax::device_mem_release(self.ptr), 0);
        }
    }
}

/// An event triggered at the end of a memory operation or kernel execution.
#[repr(C)]
pub struct Event(*mut _cl_event);

impl Event {
    /// Event is always initialized by a call to an OpenCL function (for exemple
    /// clEnqueueWriteKernel so we just have to pass a null pointer (more precisely, a pointer to a
    /// null pointer)
    fn new() -> Self {
        let event: *mut _cl_event = std::ptr::null_mut();
        Event(event)
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            telajax::event_release(self.0);
        }
    }
}

/// Calls the closure passed in data.
unsafe extern "C" fn callback_wrapper<F: FnOnce()>(
    _: cl_event,
    _: i32,
    data: *mut std::os::raw::c_void,
) {
    let data = Box::from_raw(data as *mut CallbackData<F>);
    (data.closure)();
}

type Callback = unsafe extern "C" fn(*const libc::c_void, i32, *mut libc::c_void);

struct CallbackData<F: FnOnce()> {
    closure: F,
    arc_device: Arc<device_t>,
}
