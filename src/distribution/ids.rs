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

pub fn index_to_fake_uuid(id: usize) -> Uuid {
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
