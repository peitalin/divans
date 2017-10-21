#![no_std]
#[cfg(test)]
#[macro_use]
extern crate std;

#[cfg(not(test))]
#[cfg(feature="billing")]
#[macro_use]
extern crate std;
extern crate alloc_no_stdlib as alloc;
extern crate brotli_decompressor;

pub mod interface;
pub mod slice_util;
mod probability;
#[macro_use]
mod priors;
#[macro_use]
mod encoder;
mod debug_encoder;
mod cmd_to_raw;
mod raw_to_cmd;
mod codec;
mod cmd_to_divans;
mod divans_to_raw;
mod billing;
mod ans;
pub mod constants;
pub use brotli_decompressor::{BrotliResult};
pub use alloc::{AllocatedStackMemory, Allocator, SliceWrapper, SliceWrapperMut, StackAllocator};
pub use interface::{BlockSwitch, LiteralBlockSwitch, Command, Compressor, CopyCommand, Decompressor, DictCommand, LiteralCommand, Nop, NewWithAllocator, ArithmeticEncoderOrDecoder, LiteralPredictionModeNibble, PredictionModeContextMap};
pub use cmd_to_raw::DivansRecodeState;
pub use codec::CMD_BUFFER_SIZE;
pub use divans_to_raw::DecoderSpecialization;
pub use cmd_to_divans::EncoderSpecialization;
pub use codec::{EncoderOrDecoderSpecialization, DivansCodec};
use core::marker::PhantomData;

const HEADER_LENGTH: usize = 16;
const MAGIC_NUMBER:[u8;4] = [0xff, 0xe5,0x8c, 0x9f];

pub use probability::Speed;
#[cfg(feature="blend")]
#[cfg(not(feature="debug_entropy"))]
pub type DefaultCDF16 = probability::BlendCDF16;
#[cfg(not(feature="blend"))]
#[cfg(not(feature="debug_entropy"))]
pub type DefaultCDF16 = probability::FrequentistCDF16;
#[cfg(feature="blend")]
#[cfg(feature="debug_entropy")]
pub type DefaultCDF16 = probability::DebugWrapperCDF16<probability::BlendCDF16>;
#[cfg(not(feature="blend"))]
#[cfg(feature="debug_entropy")]
pub type DefaultCDF16 = probability::DebugWrapperCDF16<probability::FrequentistCDF16>;

pub use probability::CDF2;

#[cfg(not(feature="billing"))]
macro_rules! DefaultEncoderType(
    () => {ans::EntropyEncoderANS<AllocU8>}
);

#[cfg(not(feature="billing"))]
macro_rules! DefaultDecoderType(
    () => {ans::EntropyDecoderANS<AllocU8>}
);


#[cfg(feature="billing")]
macro_rules! DefaultEncoderType(
    () => { billing::BillingArithmeticCoder<AllocU8, ans::EntropyEncoderANS<AllocU8>> }
);

#[cfg(feature="billing")]
macro_rules! DefaultDecoderType(
    () => { billing::BillingArithmeticCoder<AllocU8, ans::EntropyDecoderANS<AllocU8>> }
);

const COMPRESSOR_CMD_BUFFER_SIZE : usize = 16;
pub struct DivansCompressor<DefaultEncoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8>,
                            AllocU8:Allocator<u8>,
                            AllocU32:Allocator<u32>,
                            AllocCDF2:Allocator<probability::CDF2>,
                            AllocCDF16:Allocator<DefaultCDF16>> {
    m32: AllocU32,
    codec: DivansCodec<DefaultEncoder, EncoderSpecialization, DefaultCDF16, AllocU8, AllocCDF2, AllocCDF16>,
    header_progress: usize,
    window_size: u8,
    cmd_assembler: raw_to_cmd::RawToCmdState<AllocU8::AllocatedMemory, AllocU32>,
    cmd_array: [Command<slice_util::SliceReference<'static,u8>>; COMPRESSOR_CMD_BUFFER_SIZE],
    cmd_offset: usize,
}

pub trait DivansCompressorFactory<
     AllocU8:Allocator<u8>, 
     AllocU32:Allocator<u32>, 
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> {
     type DefaultEncoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8>;
    fn new(mut m8: AllocU8, mut m32: AllocU32, mcdf2:AllocCDF2, mcdf16:AllocCDF16,mut window_size: usize,
           literal_adaptation_rate: Option<probability::Speed>) -> 
        DivansCompressor<Self::DefaultEncoder, AllocU8, AllocU32, AllocCDF2, AllocCDF16> {
        if window_size < 10 {
            window_size = 10;
        }
        if window_size > 24 {
            window_size = 24;
        }
        let ring_buffer = m8.alloc_cell(1<<window_size);
        let enc = Self::DefaultEncoder::new(&mut m8);
        let assembler = raw_to_cmd::RawToCmdState::new(&mut m32, ring_buffer);
          DivansCompressor::<Self::DefaultEncoder, AllocU8, AllocU32, AllocCDF2, AllocCDF16> {
            m32 :m32,
            codec:DivansCodec::<Self::DefaultEncoder, EncoderSpecialization, DefaultCDF16, AllocU8, AllocCDF2, AllocCDF16>::new(
                m8,
                mcdf2,
                mcdf16,
                enc,
                EncoderSpecialization::new(),
                window_size,
                literal_adaptation_rate,
            ),
              cmd_array:[interface::Command::<slice_util::SliceReference<'static, u8>>::default(); COMPRESSOR_CMD_BUFFER_SIZE],
            cmd_offset:0,
            cmd_assembler:assembler,
            header_progress: 0,
            window_size: window_size as u8,
        }
    }
}

pub struct DivansCompressorFactoryStruct
    <AllocU8:Allocator<u8>, 
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> {
    p1: PhantomData<AllocU8>,
    p2: PhantomData<AllocCDF2>,
    p3: PhantomData<AllocCDF16>,
}

impl<AllocU8:Allocator<u8>,
     AllocU32:Allocator<u32>,
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> DivansCompressorFactory<AllocU8, AllocU32, AllocCDF2, AllocCDF16>
    for DivansCompressorFactoryStruct<AllocU8, AllocCDF2, AllocCDF16> {
     type DefaultEncoder = DefaultEncoderType!();
}

fn make_header(window_size: u8) -> [u8; HEADER_LENGTH] {
    let mut retval = [0u8; HEADER_LENGTH];
    retval[0..MAGIC_NUMBER.len()].clone_from_slice(&MAGIC_NUMBER[..]);
    retval[5] = window_size;
    retval
}

impl<DefaultEncoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8>, AllocU8:Allocator<u8>, AllocU32:Allocator<u32>, AllocCDF2:Allocator<probability::CDF2>, AllocCDF16:Allocator<DefaultCDF16>> 
    DivansCompressor<DefaultEncoder, AllocU8, AllocU32, AllocCDF2, AllocCDF16> {

    fn freeze_dry<SliceType:SliceWrapper<u8>+Default>(&mut self, input:&[Command<SliceType>]) {
        
    }
    fn write_header(&mut self, output: &mut[u8],
                    output_offset:&mut usize) -> BrotliResult {
        let bytes_avail = output.len() - *output_offset;
        if bytes_avail + self.header_progress < HEADER_LENGTH {
            output.split_at_mut(*output_offset).1.clone_from_slice(
                &make_header(self.window_size)[self.header_progress..
                                              (self.header_progress + bytes_avail)]);
            *output_offset += bytes_avail;
            return BrotliResult::NeedsMoreOutput;
        }
        output[*output_offset..(*output_offset + HEADER_LENGTH - self.header_progress)].clone_from_slice(
                &make_header(self.window_size)[self.header_progress..]);
        *output_offset += HEADER_LENGTH - self.header_progress;
        self.header_progress = HEADER_LENGTH;
        BrotliResult::ResultSuccess
    }
}

impl<DefaultEncoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8>,
     AllocU8:Allocator<u8>,
     AllocU32:Allocator<u32>,
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> Compressor for DivansCompressor<DefaultEncoder, AllocU8, AllocU32, AllocCDF2, AllocCDF16>   {
    fn encode(&mut self,
              input: &[u8],
              input_offset: &mut usize,
              output: &mut [u8],
              output_offset: &mut usize) -> BrotliResult {
//        let ret = self.cmd_assembler.stream(&mut self.codec.cross_command_state.m8, input, input_offset,
        //&mut self.cmd_array, &mut self.cmd_offset);
        let mut ret : BrotliResult = BrotliResult::ResultFailure;
        if self.cmd_offset != 0 { // we have some freeze dried items
            /*
            let mut temp_bs: [interface::Command<slice_util::SliceReference<u8>>;COMPRESSOR_CMD_BUFFER_SIZE] =
                [interface::Command::<slice_util::SliceReference<u8>>::default();COMPRESSOR_CMD_BUFFER_SIZE];
            
            match self.encode_commands(temp_bs.split_at(temp_cmd_offset).0, &mut out_cmd_offset,
                                       output, output_offset) {
            }*/
        }
        
        while true {
            let mut temp_bs: [interface::Command<slice_util::SliceReference<u8>>;COMPRESSOR_CMD_BUFFER_SIZE] =
                [interface::Command::<slice_util::SliceReference<u8>>::default();COMPRESSOR_CMD_BUFFER_SIZE];
            let mut temp_cmd_offset = 0;
            ret = self.cmd_assembler.stream(&input, input_offset,
                                            &mut temp_bs[..], &mut temp_cmd_offset);
            match ret {
                BrotliResult::NeedsMoreInput => {
                    if temp_cmd_offset == 0 {
                        // nothing to freeze dry, return
                        return ret;
                    }
                },
                BrotliResult::ResultFailure | BrotliResult::ResultSuccess => {
                    return BrotliResult::ResultFailure; // we are never done
                },
                _ => {},
            }
            /* Borrow problem
            let mut out_cmd_offset: usize = 0;
            match self.encode_commands(temp_bs.split_at(temp_cmd_offset).0, &mut out_cmd_offset,
                                  output, output_offset) {
                BrotliResult::NeedsMoreInput => {
                    match ret {
                        BrotliResult::NeedsMoreInput => return ret,
                        _ => {},
                    }
                },
                BrotliResult::NeedsMoreOutput => {
                    self.freeze_dry(temp_bs.split_at(temp_cmd_offset).0.split_at(out_cmd_offset).1);
                    return BrotliResult::NeedsMoreOutput;
                }
                BrotliResult::ResultSuccess => {
                    continue;
                }
                BrotliResult::ResultFailure => {
                    self.freeze_dry(temp_bs.split_at(temp_cmd_offset).0.split_at(out_cmd_offset).1);
                    return BrotliResult::ResultFailure;
                }
            }
*/
        }
        ret
    }
    fn encode_commands<SliceType:SliceWrapper<u8>+Default>(&mut self,
                                          input:&[Command<SliceType>],
                                          input_offset : &mut usize,
                                          output :&mut[u8],
                                          output_offset: &mut usize) -> BrotliResult{
        if self.header_progress != HEADER_LENGTH {
            match self. write_header(output, output_offset) {
                BrotliResult::ResultSuccess => {},
                res => return res,
            }
        }
        let mut unused: usize = 0;
        self.codec.encode_or_decode(&[],
                                    &mut unused,
                                    output,
                                    output_offset,
                                    input,
                                    input_offset)
    }
    fn flush(&mut self,
             output: &mut [u8],
             output_offset: &mut usize) -> BrotliResult {
        if self.header_progress != HEADER_LENGTH {
            match self.write_header(output, output_offset) {
                BrotliResult::ResultSuccess => {},
                res => return res,
            }
        }
        self.codec.flush(output, output_offset)
    }
}


pub struct HeaderParser<AllocU8:Allocator<u8>,
                        AllocCDF2:Allocator<probability::CDF2>,
                        AllocCDF16:Allocator<DefaultCDF16>> {
    header:[u8;HEADER_LENGTH],
    read_offset: usize,
    m8: Option<AllocU8>,
    mcdf2: Option<AllocCDF2>,
    mcdf16: Option<AllocCDF16>,
}
impl<AllocU8:Allocator<u8>,
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>>HeaderParser<AllocU8, AllocCDF2, AllocCDF16> {
    pub fn parse_header(&mut self)->Result<usize, BrotliResult>{
        if self.header[0] != MAGIC_NUMBER[0] ||
            self.header[1] != MAGIC_NUMBER[1] ||
            self.header[2] != MAGIC_NUMBER[2] ||
            self.header[3] != MAGIC_NUMBER[3] {
                return Err(BrotliResult::ResultFailure);
            }
        let window_size = self.header[5] as usize;
        if window_size < 10 || window_size > 25 {
            return Err(BrotliResult::ResultFailure);
        }
        Ok(window_size)
    }

}

pub enum DivansDecompressor<DefaultDecoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8>,
                            AllocU8:Allocator<u8>,
                            AllocCDF2:Allocator<probability::CDF2>,
                            AllocCDF16:Allocator<DefaultCDF16>> {
    Header(HeaderParser<AllocU8, AllocCDF2, AllocCDF16>),
    Decode(DivansCodec<DefaultDecoder, DecoderSpecialization, DefaultCDF16, AllocU8, AllocCDF2, AllocCDF16>, usize),
}

pub trait DivansDecompressorFactory<
     AllocU8:Allocator<u8>, 
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> {
     type DefaultDecoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8>;
    fn new(m8: AllocU8, mcdf2:AllocCDF2, mcdf16:AllocCDF16) -> DivansDecompressor<Self::DefaultDecoder, AllocU8, AllocCDF2, AllocCDF16> {
        DivansDecompressor::Header(HeaderParser{header:[0u8;HEADER_LENGTH], read_offset:0, m8:Some(m8), mcdf2:Some(mcdf2), mcdf16:Some(mcdf16)})
    }
}

impl<DefaultDecoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8> + interface::BillingCapability,
                        AllocU8:Allocator<u8>,
                        AllocCDF2:Allocator<probability::CDF2>,
                        AllocCDF16:Allocator<DefaultCDF16>>  
    DivansDecompressor<DefaultDecoder, AllocU8, AllocCDF2, AllocCDF16> {

    fn finish_parsing_header(&mut self, window_size: usize) -> BrotliResult {
        if window_size < 10 {
            return BrotliResult::ResultFailure;
        }
        if window_size > 24 {
            return BrotliResult::ResultFailure;
        }
        let mut m8:AllocU8;
        let mcdf2:AllocCDF2;
        let mcdf16:AllocCDF16;
        match self {
            &mut DivansDecompressor::Header(ref mut header) => {
                m8 = match core::mem::replace(&mut header.m8, None) {
                    None => return BrotliResult::ResultFailure,
                    Some(m) => m,
                }
            },
            _ => return BrotliResult::ResultFailure,
        }
        match self {
            &mut DivansDecompressor::Header(ref mut header) => {
                mcdf2 = match core::mem::replace(&mut header.mcdf2, None) {
                    None => return BrotliResult::ResultFailure,
                    Some(m) => m,
                }
            },
            _ => return BrotliResult::ResultFailure,
        }
        match self {
            &mut DivansDecompressor::Header(ref mut header) => {
                mcdf16 = match core::mem::replace(&mut header.mcdf16, None) {
                    None => return BrotliResult::ResultFailure,
                    Some(m) => m,
                }
            },
            _ => return BrotliResult::ResultFailure,
        }
        //update this if you change the SelectedArithmeticDecoder macro
        let decoder = DefaultDecoder::new(&mut m8);
        core::mem::replace(self,
                           DivansDecompressor::Decode(DivansCodec::<DefaultDecoder,
                                                                    DecoderSpecialization,
                                                                    DefaultCDF16,
                                                                    AllocU8,
                                                                    AllocCDF2,
                                                                    AllocCDF16>::new(m8,
                                                                                     mcdf2,
                                                                                     mcdf16,
                                                                                     decoder,
                                                                                     DecoderSpecialization::new(),
                                                                                     window_size,
                                                                                     None), 0));
        BrotliResult::ResultSuccess
    }
    pub fn free(self) -> (AllocU8, AllocCDF2, AllocCDF16) {
        match self {
            DivansDecompressor::Header(parser) => {
                (parser.m8.unwrap(),
                 parser.mcdf2.unwrap(),
                 parser.mcdf16.unwrap())
            },
            DivansDecompressor::Decode(decoder, bytes_encoded) => {
                decoder.get_coder().debug_print(bytes_encoded);
                decoder.free()
            }
        }
    }
}

impl<DefaultDecoder: ArithmeticEncoderOrDecoder + NewWithAllocator<AllocU8> + interface::BillingCapability,
     AllocU8:Allocator<u8>,
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> Decompressor for DivansDecompressor<DefaultDecoder, AllocU8, AllocCDF2, AllocCDF16> {
    fn decode(&mut self,
              input:&[u8],
              input_offset:&mut usize,
              output:&mut [u8],
              output_offset: &mut usize) -> BrotliResult {
        let window_size: usize;

        match self  {
            &mut DivansDecompressor::Header(ref mut header_parser) => {
                let remaining = input.len() - *input_offset;
                let header_left = header_parser.header.len() - header_parser.read_offset;
                if remaining >= header_left {
                    header_parser.header[header_parser.read_offset..].clone_from_slice(
                        input.split_at(*input_offset).1.split_at(header_left).0);
                    *input_offset += header_left;
                    match header_parser.parse_header() {
                        Ok(wsize) => window_size = wsize,
                        Err(result) => return result,
                    }
                } else {
                    header_parser.header[(header_parser.read_offset)..
                                         (header_parser.read_offset+remaining)].clone_from_slice(
                        input.split_at(*input_offset).1);
                    *input_offset += remaining;
                    header_parser.read_offset += remaining;
                    return BrotliResult::NeedsMoreInput;
                }
            },
            &mut DivansDecompressor::Decode(ref mut divans_parser, ref mut bytes_encoded) => {
                let mut unused:usize = 0;
                let old_output_offset = *output_offset;
                let retval = divans_parser.encode_or_decode::<AllocU8::AllocatedMemory>(
                    input,
                    input_offset,
                    output,
                    output_offset,
                    &[],
                    &mut unused);
                *bytes_encoded += *output_offset - old_output_offset;
                return retval;
            },
        }
        self.finish_parsing_header(window_size);
        if *input_offset < input.len() {
            return self.decode(input, input_offset, output, output_offset);
        }
        BrotliResult::NeedsMoreInput
    }
}

pub struct DivansDecompressorFactoryStruct
    <AllocU8:Allocator<u8>, 
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> {
    p1: PhantomData<AllocU8>,
    p2: PhantomData<AllocCDF2>,
    p3: PhantomData<AllocCDF16>,
}

impl<AllocU8:Allocator<u8>, 
     AllocCDF2:Allocator<probability::CDF2>,
     AllocCDF16:Allocator<DefaultCDF16>> DivansDecompressorFactory<AllocU8, AllocCDF2, AllocCDF16>
    for DivansDecompressorFactoryStruct<AllocU8, AllocCDF2, AllocCDF16> {
     type DefaultDecoder = DefaultDecoderType!();
}


