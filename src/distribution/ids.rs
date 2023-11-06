// Copyright 2023 Xayn AG
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::iter;

use uuid::Uuid;

pub fn index_to_fake_uuid(id: u64) -> Uuid {
    let mut bytes = [0u8; 16];
    for (idx, byte) in iter::repeat(id.to_be_bytes())
        .flatten()
        .enumerate()
        .take(16)
    {
        bytes[idx] = byte;
    }
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

pub fn fake_uuid_to_index(uuid: Uuid) -> u64 {
    let mut bytes: [u8; 8] = uuid.as_bytes()[8..].try_into().unwrap();
    // remove uuid type/version flags
    bytes[0] = 0;
    u64::from_be_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_to_uuid_conversion_is_okay() {
        assert_eq!(1 << 54, fake_uuid_to_index(index_to_fake_uuid(1 << 54)));
        assert_eq!(
            (1 << 54) - 1,
            fake_uuid_to_index(index_to_fake_uuid((1 << 54) - 1))
        );
        assert_eq!(1, fake_uuid_to_index(index_to_fake_uuid(1)));
        assert_eq!(0, fake_uuid_to_index(index_to_fake_uuid(0)));
    }
}
