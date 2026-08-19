//! TEST-03 / S-6: el pipeline de decodificación completo contra entrada
//! arbitraria. Criterio: cero panics. Todo lo que llega del MCU es entrada
//! no confiable (SEC-04).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // COBS + CRC + header de frame + separación de records
    if let Ok((_header, recs)) = mole_codec::frame::decode_wire(data) {
        // decodificación tipada de cada record + catálogo + args
        let mut cat = mole_codec::catalog::Catalog::new();
        for r in &recs {
            if let Ok(rec) =
                mole_codec::record::Record::decode_payload(r.rtype, &r.payload, &cat.types)
            {
                let _ = cat.apply(&rec);
                if let mole_codec::record::Record::LogFmt { fmt_id, args_raw, .. } = &rec {
                    let _ = cat.parse_args(*fmt_id, args_raw);
                }
            }
        }
    }
});
