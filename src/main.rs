use anyhow::{bail, Context, Result};
use byteorder::{BigEndian, ReadBytesExt};
use clap::clap_app;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

#[derive(Debug)]
enum ValueType {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<NBTValue>),
    Compound(HashMap<String, NBTValue>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

#[derive(Debug)]
struct NBTValue {
    start: usize,
    end: usize,
    ty: ValueType,
}

impl NBTValue {
    fn size(&self) -> usize {
        self.end - self.start
    }
}

struct NBTReader {
    buffer: Cursor<Vec<u8>>,
}

impl NBTReader {
    const READ_FNS: &'static [fn(&mut Self) -> Result<ValueType>] = &[
        Self::read_zero,
        Self::read_byte,
        Self::read_short,
        Self::read_int,
        Self::read_long,
        Self::read_float,
        Self::read_double,
        Self::read_byte_array,
        Self::read_string,
        Self::read_list,
        Self::read_compound,
        Self::read_int_array,
        Self::read_long_array,
    ];

    fn new(data: Vec<u8>) -> Self {
        Self {
            buffer: Cursor::new(data),
        }
    }

    fn read(&mut self) -> Result<NBTValue> {
        self.read_value(10)
    }

    fn read_value(&mut self, type_id: usize) -> Result<NBTValue> {
        let reader = Self::READ_FNS[type_id];
        let start = self.buffer.position() as usize;
        let inner = reader(self)?;
        let end = self.buffer.position() as usize;
        Ok(NBTValue {
            start,
            end,
            ty: inner,
        })
    }

    fn read_zero(&mut self) -> Result<ValueType> {
        unreachable!("Tried to read value with type id 0");
    }

    fn read_byte(&mut self) -> Result<ValueType> {
        Ok(ValueType::Byte(self.buffer.read_i8()?))
    }

    fn read_short(&mut self) -> Result<ValueType> {
        Ok(ValueType::Short(self.buffer.read_i16::<BigEndian>()?))
    }

    fn read_int(&mut self) -> Result<ValueType> {
        Ok(ValueType::Int(self.buffer.read_i32::<BigEndian>()?))
    }

    fn read_long(&mut self) -> Result<ValueType> {
        Ok(ValueType::Long(self.buffer.read_i64::<BigEndian>()?))
    }

    fn read_float(&mut self) -> Result<ValueType> {
        Ok(ValueType::Float(self.buffer.read_f32::<BigEndian>()?))
    }

    fn read_double(&mut self) -> Result<ValueType> {
        Ok(ValueType::Double(self.buffer.read_f64::<BigEndian>()?))
    }

    fn read_byte_array(&mut self) -> Result<ValueType> {
        let length = self.buffer.read_i32::<BigEndian>()?;
        let mut items = Vec::new();
        for _ in 0..length {
            items.push(self.buffer.read_i8()?);
        }
        Ok(ValueType::ByteArray(items))
    }

    fn read_string(&mut self) -> Result<ValueType> {
        let length = self.buffer.read_i16::<BigEndian>()?;
        let mut bytes = vec![0; length as usize];
        self.buffer.read_exact(&mut bytes)?;
        let string = String::from_utf8(bytes)?;
        Ok(ValueType::String(string))
    }

    fn read_name(&mut self) -> Result<String> {
        let length = self.buffer.read_i16::<BigEndian>()?;
        let mut bytes = vec![0; length as usize];
        self.buffer.read_exact(&mut bytes)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn read_list(&mut self) -> Result<ValueType> {
        let type_id = self.buffer.read_i8()? as usize;
        let length = self.buffer.read_i32::<BigEndian>()?;
        let mut items = Vec::new();
        for _ in 0..length {
            items.push(self.read_value(type_id)?);
        }
        Ok(ValueType::List(items))
    }

    fn read_compound(&mut self) -> Result<ValueType> {
        let mut compound = HashMap::new();
        loop {
            let type_id = self.buffer.read_i8().unwrap() as usize;
            if type_id == 0 {
                return Ok(ValueType::Compound(compound));
            }
            let name = self.read_name()?;
            compound.insert(name, self.read_value(type_id)?);
        }
    }

    fn read_int_array(&mut self) -> Result<ValueType> {
        let length = self.buffer.read_i32::<BigEndian>()?;
        let mut items = Vec::new();
        for _ in 0..length {
            items.push(self.buffer.read_i32::<BigEndian>()?);
        }
        Ok(ValueType::IntArray(items))
    }

    fn read_long_array(&mut self) -> Result<ValueType> {
        let length = self.buffer.read_i32::<BigEndian>()?;
        let mut items = Vec::new();
        for _ in 0..length {
            items.push(self.buffer.read_i64::<BigEndian>()?);
        }
        Ok(ValueType::LongArray(items))
    }
}

macro_rules! get_variant {
    ($expression:expr, $variant:path) => {
        match &$expression {
            $variant(x) => x,
            _ => {
                bail!("incorrect variant")
            }
        }
    };
}

fn get_input() -> Result<String> {
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer)?;
    Ok(buffer)
}

struct ItemEntry {
    index: usize,
    size: usize,
    start: usize,
    end: usize,
}

fn main() -> Result<()> {
    let matches = clap_app!(large_nbt_fixer =>
        (version: "1.0")
        (author: "StackDoubleFlow <ojaslandge@gmail.com>")
        (about: "Removes large nbt from player.dat files")
        (@arg input: +required "The player.dat file to modify")
    )
    .get_matches();

    let path = Path::new(matches.value_of("input").context("input arg missing")?);
    let file = File::open(path)?;
    let mut data = Vec::new();
    GzDecoder::new(file).read_to_end(&mut data)?;

    // The root compound doesn't have an end tag?
    data.push(0);

    let nbt = NBTReader::new(data.clone()).read()?;
    let root = get_variant!(nbt.ty, ValueType::Compound);
    let compound = get_variant!(root[""].ty, ValueType::Compound);
    let inventory = get_variant!(compound["Inventory"].ty, ValueType::List);

    let mut items = Vec::new();
    for (index, entry) in inventory.iter().enumerate() {
        items.push(ItemEntry {
            index,
            size: entry.size(),
            start: entry.start,
            end: entry.end,
        })
    }

    if items.is_empty() {
        bail!("Inventory is empty");
    }

    let size = compound["Inventory"].size();
    println!("Total inventory size is {} bytes", size);
    println!("All inventory items ranked by size:");
    items.sort_by_key(|item| item.size);
    items.reverse();
    for item in &items {
        println!("Slot {}: {} bytes", item.index, item.size);
    }

    print!("Which slot would you like to delete? ");
    std::io::stdout().flush()?;
    let n = get_input()?.trim().parse::<usize>()?;

    let item = items
        .iter()
        .find(|item| item.index == n)
        .context("Slot not found")?;
    println!("Deleting item...");
    data.drain(item.start..item.end);

    println!("Compressing...");
    let file = File::create(path)?;
    let mut encoder = GzEncoder::new(file, Compression::new(9));
    encoder.write_all(&data)?;

    println!("Done! New inventory size is {}", size - item.size + 1);
    Ok(())
}
