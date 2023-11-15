// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { createContext, type ReactNode } from 'react';

type ObjectAddressContextProps = {
	address: string;
};

const ObjectAddressContext = createContext<ObjectAddressContextProps | null>(null);

export function ObjectAddressProvider({ children }: { children: ReactNode }) {
	return (
		<ObjectAddressContext.Provider
			value={{
				address: '',
			}}
		>
			{children}
		</ObjectAddressContext.Provider>
	);
}
